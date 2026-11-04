-- Public feedback. No IP address is stored: `sender` is an HMAC of it with a secret key.
CREATE TABLE IF NOT EXISTS notes (
  id TEXT PRIMARY KEY,
  hash TEXT NOT NULL UNIQUE,
  sender TEXT NOT NULL,
  -- pending, then accepted or rejected. A handled note keeps only its hash, so the same text
  -- does not come back; its text and context are emptied.
  status TEXT NOT NULL DEFAULT 'pending',
  tag TEXT NOT NULL,
  text TEXT NOT NULL,
  context TEXT NOT NULL,
  created INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS notes_by_status ON notes (status, created);
CREATE INDEX IF NOT EXISTS notes_by_sender ON notes (sender, status);

-- Who reported a note, so one sender cannot inflate its count.
CREATE TABLE IF NOT EXISTS reports (
  note TEXT NOT NULL,
  sender TEXT NOT NULL,
  PRIMARY KEY (note, sender)
);

-- Fixed-window counters: per sender and for the whole service.
CREATE TABLE IF NOT EXISTS counters (
  key TEXT NOT NULL,
  window INTEGER NOT NULL,
  count INTEGER NOT NULL,
  PRIMARY KEY (key, window)
);

CREATE TABLE IF NOT EXISTS blocked (
  sender TEXT PRIMARY KEY,
  created INTEGER NOT NULL
);

-- Usage reports people opted in to: totals per day. Nothing here links a row to a sender. An empty
-- `subject` means the event has none, since a primary key column cannot usefully be NULL.
CREATE TABLE IF NOT EXISTS usage_daily (
  day INTEGER NOT NULL,
  version TEXT NOT NULL,
  os TEXT NOT NULL,
  os_version TEXT NOT NULL,
  chip TEXT NOT NULL,
  kind TEXT NOT NULL,
  subject TEXT NOT NULL,
  result TEXT NOT NULL,
  count INTEGER NOT NULL,
  PRIMARY KEY (day, version, os, os_version, chip, kind, subject, result)
);

-- Who reported today, to count them once. The daily clean-up turns finished days into a number in
-- `usage_reach` and deletes these rows.
CREATE TABLE IF NOT EXISTS usage_senders (
  day INTEGER NOT NULL,
  sender TEXT NOT NULL,
  PRIMARY KEY (day, sender)
);

CREATE TABLE IF NOT EXISTS usage_reach (
  day INTEGER PRIMARY KEY,
  senders INTEGER NOT NULL
);
