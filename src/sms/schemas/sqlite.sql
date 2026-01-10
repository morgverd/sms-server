CREATE TABLE IF NOT EXISTS messages (
    message_id INTEGER PRIMARY KEY AUTOINCREMENT,
    phone_number TEXT NOT NULL,
    message_content TEXT NOT NULL,
    message_reference INTEGER CHECK (message_reference >= 0 AND message_reference <= 255),
    is_outgoing BOOLEAN NOT NULL,
    status INTEGER DEFAULT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    completed_at INTEGER DEFAULT NULL
);

CREATE TABLE IF NOT EXISTS friendly_names (
    phone_number TEXT PRIMARY KEY,
    friendly_name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS send_failures (
    message_id INTEGER PRIMARY KEY,
    error_message TEXT NOT NULL,
    created_at INTEGER DEFAULT (unixepoch()),
    FOREIGN KEY (message_id) REFERENCES messages(message_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS delivery_reports (
    report_id INTEGER PRIMARY KEY,
    message_id INTEGER NOT NULL,
    status INTEGER NOT NULL,
    is_final BOOLEAN NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY (message_id) REFERENCES messages(message_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_messages_phone_number ON messages(phone_number);
CREATE INDEX IF NOT EXISTS idx_messages_status ON messages(status);
CREATE INDEX IF NOT EXISTS idx_messages_is_outgoing ON messages(is_outgoing);
CREATE INDEX IF NOT EXISTS idx_messages_created_at ON messages(created_at);
CREATE INDEX IF NOT EXISTS idx_messages_completed_at ON messages(completed_at);
CREATE INDEX IF NOT EXISTS idx_friendly_name ON friendly_names(friendly_name);