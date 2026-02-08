#[derive(Debug)]
pub enum LineEvent {
    Line(String),
    Prompt(String),
}

pub struct LineBuffer {
    buffer: Vec<u8>,
    max_buffer_size: usize,
}
impl LineBuffer {
    pub fn with_max_size(size: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(512),
            max_buffer_size: size,
        }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn process_data(&mut self, data: &[u8]) -> Vec<LineEvent> {
        self.buffer.extend_from_slice(data);

        let mut events = Vec::new();
        let mut start = 0;
        let mut i = 0;

        while i < self.buffer.len() {
            match self.buffer[i] {
                b'\r' | b'\n' => {
                    if i > start {
                        if let Some(line_event) =
                            self.try_create_event(&self.buffer[start..i], LineEvent::Line)
                        {
                            events.push(line_event);
                        }
                    }

                    // Skip all consecutive newlines.
                    while i < self.buffer.len()
                        && (self.buffer[i] == b'\r' || self.buffer[i] == b'\n')
                    {
                        i += 1;
                    }
                    start = i;
                }
                b'>' => {
                    // Only treat as prompt if it's at start of line or after whitespace.
                    let is_prompt = start == i
                        || (i > 0 && (self.buffer[i - 1] == b'\n' || self.buffer[i - 1] == b'\r'));

                    if is_prompt {
                        // Consume '>' and any trailing whitespace (e.g. "> ")
                        let prompt_start = start;
                        i += 1;
                        while i < self.buffer.len() && self.buffer[i] == b' ' {
                            i += 1;
                        }

                        if let Some(prompt_event) =
                            self.try_create_event(&self.buffer[prompt_start..i], LineEvent::Prompt)
                        {
                            events.push(prompt_event);
                        }
                        start = i;
                        continue;
                    } else {
                        i += 1;
                    }
                }
                _ => i += 1,
            }
        }

        // Retain any partial line at the end.
        if start > 0 {
            self.buffer.drain(..start);
        }

        if self.buffer.len() > self.max_buffer_size {
            let keep_from = self.buffer.len().saturating_sub(self.max_buffer_size);

            // Search within [0..keep_from] for the last newline, then trim up to
            // and including it so the kept portion starts on a clean line boundary.
            // Falls back to the raw offset if no newline exists.
            let trim_to = self.buffer[..keep_from]
                .iter()
                .rposition(|&b| b == b'\n')
                .map(|pos| pos + 1)
                .unwrap_or(keep_from);

            self.buffer.drain(..trim_to);
        }

        events
    }

    fn try_create_event<F>(&self, data: &[u8], constructor: F) -> Option<LineEvent>
    where
        F: FnOnce(String) -> LineEvent,
    {
        // Ignore if empty or whitespace only.
        if data.is_empty() || data.iter().all(|&b| b.is_ascii_whitespace()) {
            return None;
        }

        let content = match std::str::from_utf8(data) {
            Ok(content) => content.trim(),
            Err(_) => {
                // Handle invalid UTF-8 gracefully - convert with replacement chars
                return match String::from_utf8_lossy(data).trim() {
                    trimmed if !trimmed.is_empty() => Some(constructor(trimmed.to_string())),
                    _ => None,
                };
            }
        };

        if !content.is_empty() {
            Some(constructor(content.to_string()))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_line_processing() {
        let mut buffer = LineBuffer::with_max_size(1024);

        let events = buffer.process_data(b"hello world\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "hello world"));

        let events = buffer.process_data(b"first\nsecond\nthird\n");
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "first"));
        assert!(matches!(&events[1], LineEvent::Line(s) if s == "second"));
        assert!(matches!(&events[2], LineEvent::Line(s) if s == "third"));
    }

    #[test]
    fn test_prompt_detection() {
        let mut buffer = LineBuffer::with_max_size(1024);

        let events = buffer.process_data(b">");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Prompt(s) if s == ">"));

        let events = buffer.process_data(b"output\n>");
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "output"));
        assert!(matches!(&events[1], LineEvent::Prompt(s) if s == ">"));

        let events = buffer.process_data(b"test>data\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "test>data"));
    }

    #[test]
    fn test_prompt_with_trailing_space() {
        let mut buffer = LineBuffer::with_max_size(1024);

        let events = buffer.process_data(b"> ");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Prompt(s) if s == ">"));

        buffer.clear();
        let events = buffer.process_data(b"OK\r\n> ");
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "OK"));
        assert!(matches!(&events[1], LineEvent::Prompt(s) if s == ">"));

        buffer.clear();
        let events = buffer.process_data(b">   ");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Prompt(s) if s == ">"));

        buffer.clear();
        let events = buffer.process_data(b"> PDU_DATA\n");
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], LineEvent::Prompt(s) if s == ">"));
        assert!(matches!(&events[1], LineEvent::Line(s) if s == "PDU_DATA"));
    }

    #[test]
    fn test_mixed_events_sequence() {
        let mut buffer = LineBuffer::with_max_size(1024);

        let events = buffer.process_data(b"command output\n>user input\n>");
        assert_eq!(events.len(), 4);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "command output"));
        assert!(matches!(&events[1], LineEvent::Prompt(s) if s == ">"));
        assert!(matches!(&events[2], LineEvent::Line(s) if s == "user input"));
        assert!(matches!(&events[3], LineEvent::Prompt(s) if s == ">"));
    }

    #[test]
    fn test_incremental_processing() {
        let mut buffer = LineBuffer::with_max_size(1024);

        assert_eq!(buffer.process_data(b"partial").len(), 0);
        assert_eq!(buffer.process_data(b" data").len(), 0);

        let events = buffer.process_data(b" here\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "partial data here"));

        assert_eq!(buffer.process_data(b"line").len(), 0);
        assert_eq!(buffer.process_data(b" two").len(), 0);
        let events = buffer.process_data(b"\n>");
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "line two"));
        assert!(matches!(&events[1], LineEvent::Prompt(s) if s == ">"));

        let events = buffer.process_data(b"command\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "command"));
    }

    #[test]
    fn test_line_endings() {
        let mut buffer = LineBuffer::with_max_size(1024);

        let events = buffer.process_data(b"unix\nwindows\r\nmac\rend\n");
        assert_eq!(events.len(), 4);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "unix"));
        assert!(matches!(&events[1], LineEvent::Line(s) if s == "windows"));
        assert!(matches!(&events[2], LineEvent::Line(s) if s == "mac"));
        assert!(matches!(&events[3], LineEvent::Line(s) if s == "end"));

        let events = buffer.process_data(b"output\r>");
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "output"));
        assert!(matches!(&events[1], LineEvent::Prompt(s) if s == ">"));
    }

    #[test]
    fn test_whitespace_handling() {
        let mut buffer = LineBuffer::with_max_size(1024);

        let events = buffer.process_data(b"  hello world  \n");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "hello world"));

        let events = buffer.process_data(b"\n\n   \n\t\t\n");
        assert_eq!(events.len(), 0);

        let events = buffer.process_data(b"line1\n\n\nline2\n");
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "line1"));
        assert!(matches!(&events[1], LineEvent::Line(s) if s == "line2"));
    }

    #[test]
    fn test_buffer_size_limits_line_boundary() {
        let mut buffer = LineBuffer::with_max_size(20);

        let events = buffer.process_data(b"short\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "short"));

        let events = buffer.process_data(b"this is a longer line\n");
        assert!(events
            .iter()
            .any(|e| matches!(e, LineEvent::Line(s) if s == "this is a longer line")));
    }

    #[test]
    fn test_buffer_size_limits_partial_line_truncation() {
        let mut buffer = LineBuffer::with_max_size(10);

        buffer.process_data(b"0123456789ABCDEFGHIJ");
        assert!(buffer.buffer.len() <= 10);
    }

    #[test]
    fn test_buffer_size_limits_newline_aligned_truncation() {
        let mut buffer = LineBuffer::with_max_size(15);

        let events = buffer.process_data(b"done\n");
        assert_eq!(events.len(), 1);

        let events = buffer.process_data(b"AAAA\nBBBBBBBBBBBBBBB");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "AAAA"));
        assert_eq!(buffer.buffer.len(), 15);
    }

    #[test]
    fn test_invalid_utf8_recovery() {
        let mut buffer = LineBuffer::with_max_size(1024);

        let events = buffer.process_data(&[0xFF, 0xFE, 0xFD, b'\n']);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Line(_)));
    }

    #[test]
    fn test_clear_buffer() {
        let mut buffer = LineBuffer::with_max_size(1024);

        buffer.process_data(b"some data");
        assert!(!buffer.buffer.is_empty());

        buffer.clear();
        assert!(buffer.buffer.is_empty());

        let events = buffer.process_data(b"new line\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LineEvent::Line(s) if s == "new line"));
    }
}
