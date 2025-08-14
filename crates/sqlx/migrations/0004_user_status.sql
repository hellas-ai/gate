-- Add enabled and disabled_at fields to users table
ALTER TABLE users ADD COLUMN enabled BOOLEAN NOT NULL DEFAULT 1;
ALTER TABLE users ADD COLUMN disabled_at TIMESTAMP;

-- Index for filtering by enabled status
CREATE INDEX IF NOT EXISTS idx_users_enabled ON users(enabled);