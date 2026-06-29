-- V014__agent_id.sql
-- RFC 10181 Phase 2: Add agent_id to perception events for attribution.
-- NULL means "from the user/sidebar" (no agent session).
-- Non-NULL is the agent's session URI (chatSessionResource).

ALTER TABLE inbox_data ADD COLUMN agent_id TEXT;
