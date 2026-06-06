-- Adds support for batch conversions.
--
-- `conversion.synthetic` distinguishes rows produced by a batch run that simply
-- reference an existing file (either an already-suitable source or a prior
-- non-synthetic conversion) from rows that own a freshly-converted file.
-- `list_for_ebook` filters synthetic rows so the per-ebook conversion UI is
-- unaffected.
--
-- `conversion_batch.for_entity` accepted values: 'BOOKSHELF', 'SERIES', 'AUTHOR'.

ALTER TABLE conversion ADD COLUMN synthetic INTEGER NOT NULL DEFAULT 0;

CREATE INDEX ix_conversion_batch_id ON conversion(batch_id);
CREATE INDEX ix_conversion_batch_created_by ON conversion_batch(created_by);
