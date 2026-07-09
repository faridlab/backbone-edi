-- Down: drop enum types for edi module
DROP TYPE IF EXISTS partner_direction CASCADE;
DROP TYPE IF EXISTS edi_format CASCADE;
DROP TYPE IF EXISTS edi_status CASCADE;
DROP TYPE IF EXISTS edi_direction CASCADE;
DROP TYPE IF EXISTS edi_doc_type CASCADE;
