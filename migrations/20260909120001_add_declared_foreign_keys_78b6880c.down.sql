-- Down: drop the declared foreign keys for edi module
ALTER TABLE IF EXISTS edi.edi_documents DROP CONSTRAINT IF EXISTS fk_edi_documents_partner_id;
