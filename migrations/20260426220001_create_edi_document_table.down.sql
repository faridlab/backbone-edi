-- Down: drop edi.edi_documents table
DROP TABLE IF EXISTS edi.edi_documents CASCADE;
DROP FUNCTION IF EXISTS edi.edi_documents_audit_timestamp() CASCADE;
