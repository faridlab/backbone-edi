-- Add the UBL BIS 3 order format to the edi_format enum (inbound order import).
-- The type name stays unqualified to match how the initial create_enums migration
-- created it (public schema). IF NOT EXISTS keeps the migration idempotent.
ALTER TYPE edi_format ADD VALUE IF NOT EXISTS 'ubl_bis3';
