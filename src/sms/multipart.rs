use anyhow::anyhow;
use anyhow::Result;
use sms_types::sms::{SmsIncomingMessage, SmsMessage};
use std::time::Duration;
use tokio::time::Instant;
use tracing::log::debug;

const MULTIPART_MESSAGES_STALLED_DURATION: Duration = Duration::from_secs(30 * 60); // 30 minutes

#[derive(Debug, Clone)]
pub struct SMSMultipartHeader {
    pub message_reference: u8,
    pub total: u8,
    pub index: u8,
}

#[derive(Debug, Clone)]
pub struct SMSMultipartMessages {
    total_size: usize,
    last_updated: Instant,
    first_message: Option<SmsIncomingMessage>,
    text_len: usize,
    text_parts: Vec<Option<String>>,
    received_count: usize,
}
impl SMSMultipartMessages {
    pub fn with_capacity(total_size: usize) -> Self {
        Self {
            total_size,
            last_updated: Instant::now(),
            first_message: None,
            text_len: 0,
            text_parts: vec![None; total_size],
            received_count: 0,
        }
    }

    pub fn add_message(&mut self, message: SmsIncomingMessage, index: u8) -> bool {
        self.last_updated = Instant::now();

        let idx = (index as usize).saturating_sub(1);
        if idx < self.text_parts.len() && self.text_parts[idx].is_none() {
            // Remove message separator char.
            let content = if message.content.ends_with("@") {
                message
                    .content
                    .strip_suffix("@")
                    .unwrap_or(&message.content)
                    .to_string()
            } else {
                message.content.to_string()
            };

            self.text_len += content.len();
            self.text_parts[idx] = Some(content);
            self.received_count += 1;
        }

        if self.first_message.is_none() {
            self.first_message = Some(message);
        }

        debug!(
            "Received Multipart SMS Count: {:?} | Max: {:?}",
            self.received_count, self.total_size
        );
        self.received_count >= self.total_size
    }

    pub fn compile(&self) -> Result<SmsMessage> {
        let first_message = match self.first_message.as_ref() {
            Some(first_message) => first_message,
            None => {
                return Err(anyhow!(
                    "Missing required first message to convert into SMSMessage!"
                ))
            }
        };

        let mut content = String::with_capacity(self.text_len);
        for text in self.text_parts.iter().flatten() {
            content.push_str(text);
        }

        let mut message = SmsMessage::from(first_message);
        message.message_content = content;

        Ok(message)
    }

    #[inline]
    pub fn is_stalled(&self) -> bool {
        self.last_updated.elapsed() > MULTIPART_MESSAGES_STALLED_DURATION
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEST_NUMBER: &str = "+123456789";

    fn create_test_message(content: &str) -> SmsIncomingMessage {
        SmsIncomingMessage {
            phone_number: TEST_NUMBER.to_string(),
            user_data_header: None,
            content: content.to_string(),
        }
    }

    #[test]
    fn test_multipart_assembly() {
        let mut multipart_ordered = SMSMultipartMessages::with_capacity(3);
        assert!(!multipart_ordered.add_message(create_test_message("First @"), 1));
        assert!(!multipart_ordered.add_message(create_test_message("Second @"), 2));
        assert!(multipart_ordered.add_message(create_test_message("Third"), 3));

        let result = multipart_ordered.compile().unwrap();
        assert_eq!(result.message_content, "First Second Third");

        let mut multipart_random = SMSMultipartMessages::with_capacity(5);
        assert!(!multipart_random.add_message(create_test_message("Part3 @"), 3));
        assert!(!multipart_random.add_message(create_test_message("Part5!"), 5));
        assert!(!multipart_random.add_message(create_test_message("Part1 @"), 1));
        assert!(!multipart_random.add_message(create_test_message("Part4 @"), 4));
        assert!(multipart_random.add_message(create_test_message("Part2 @"), 2));

        let result = multipart_random.compile().unwrap();
        assert_eq!(result.message_content, "Part1 Part2 Part3 Part4 Part5!");
    }

    #[test]
    fn test_special_characters() {
        let mut multipart = SMSMultipartMessages::with_capacity(8);

        multipart.add_message(create_test_message("Hello\nWorld\t@"), 1);
        multipart.add_message(create_test_message("🚀🌟😀 emojis @"), 2);
        multipart.add_message(create_test_message("\"quotes\" & 'apostrophes' @"), 3);
        multipart.add_message(create_test_message("<html>&nbsp;</html> @"), 4);
        multipart.add_message(create_test_message("Ñoño José María @"), 5);
        multipart.add_message(create_test_message("Здравствуйте @"), 6);
        multipart.add_message(create_test_message("你好世界 @"), 7);
        multipart.add_message(create_test_message("Math: ∑∏∫√ End"), 8);

        let result = multipart.compile().unwrap();
        assert_eq!(
            result.message_content,
            "Hello\nWorld\t🚀🌟😀 emojis \"quotes\" & 'apostrophes' <html>&nbsp;</html> Ñoño José María Здравствуйте 你好世界 Math: ∑∏∫√ End"
        );

        let mut multipart2 = SMSMultipartMessages::with_capacity(3);
        assert_eq!(multipart2.text_len, 0);

        multipart2.add_message(create_test_message("😀😀😀@"), 1);
        let emoji_len = "😀😀😀".len();
        assert_eq!(multipart2.text_len, emoji_len);

        multipart2.add_message(create_test_message("ABC@"), 2);
        assert_eq!(multipart2.text_len, emoji_len + 3);

        multipart2.add_message(create_test_message("世界"), 3);
        let chinese_len = "世界".len();
        assert_eq!(multipart2.text_len, emoji_len + 3 + chinese_len);
    }
}
