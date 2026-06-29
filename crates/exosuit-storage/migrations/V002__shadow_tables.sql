-- V002__shadow_tables.sql
-- Rename core tables to *_data suffix for shadow table architecture
-- Per RFC 10165: shadow tables store actual data, virtual tables mediate access

-- ═══════════════════════════════════════════════════════════
-- Rename core hierarchy tables to shadow convention
-- ═══════════════════════════════════════════════════════════

ALTER TABLE epochs RENAME TO epochs_data;
ALTER TABLE phases RENAME TO phases_data;
ALTER TABLE goals RENAME TO goals_data;
ALTER TABLE tasks RENAME TO tasks_data;
ALTER TABLE phase_rfcs RENAME TO phase_rfcs_data;

-- entity_aliases is NOT renamed (it's a plain index table, not a reactive data source)
-- Per RFC 10165: "Junction tables are plain tables"
