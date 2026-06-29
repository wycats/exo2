-- V018__inbox_action_payload.sql
-- Optional machine-readable action payloads for agent-visible inbox requests.

PRAGMA foreign_keys = ON;

ALTER TABLE inbox_data ADD COLUMN action_json TEXT;
