-- Synthetic released-schema fixture; no user data.
INSERT INTO households (id, name, base_currency, created_at, updated_at)
VALUES ('11111111-1111-4111-8111-111111111111', 'Fixture Household', 'CNY', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');

INSERT INTO media_assets (id, household_id, mime_type, data, created_at)
VALUES ('22222222-2222-4222-8222-222222222222', '11111111-1111-4111-8111-111111111111', 'image/png', X'89504E470D0A1A0A', '2026-01-01T00:00:00.000Z');

INSERT INTO members (id, household_id, name, avatar_asset_id, note, sort_order, created_at, updated_at, archived_at)
VALUES
  ('33333333-3333-4333-8333-333333333333', '11111111-1111-4111-8111-111111111111', 'Active Member', '22222222-2222-4222-8222-222222222222', 'fixture member', 0, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('44444444-4444-4444-8444-444444444444', '11111111-1111-4111-8111-111111111111', 'Archived Member', NULL, NULL, 1, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z');

INSERT INTO institutions (id, household_id, name, institution_type, country_code, website, note, logo_asset_id, sort_order, created_at, updated_at, archived_at)
VALUES
  ('55555555-5555-4555-8555-555555555555', '11111111-1111-4111-8111-111111111111', 'Archived Bank', 'bank', 'CN', NULL, NULL, NULL, 0, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z'),
  ('66666666-6666-4666-8666-666666666666', '11111111-1111-4111-8111-111111111111', 'Active Bank', 'bank', 'CN', NULL, NULL, NULL, 1, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL);

INSERT INTO account_groups (id, household_id, name, icon_key, color, logo_asset_id, description, sort_order, created_at, updated_at, archived_at)
VALUES
  ('77777777-7777-4777-8777-777777777777', '11111111-1111-4111-8111-111111111111', 'Archived Group', NULL, NULL, NULL, NULL, 0, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z'),
  ('88888888-8888-4888-8888-888888888888', '11111111-1111-4111-8111-111111111111', 'Active Group', NULL, NULL, NULL, NULL, 1, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL);

INSERT INTO accounts (id, household_id, institution_id, group_id, name, primary_category, secondary_category, tracking_mode, default_currency, note, logo_asset_id, include_in_net_worth, include_in_investment, include_in_liquid_assets, opened_on, closed_on, sort_order, created_at, updated_at, archived_at)
VALUES
  ('99999999-9999-4999-8999-999999999999', '11111111-1111-4111-8111-111111111111', '55555555-5555-4555-8555-555555555555', '77777777-7777-4777-8777-777777777777', 'Legacy Balance', 'cash_equivalent', 'bank_account', 'balance', 'CNY', 'retained archived references', NULL, 1, 0, 1, NULL, NULL, 0, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', '11111111-1111-4111-8111-111111111111', '66666666-6666-4666-8666-666666666666', '88888888-8888-4888-8888-888888888888', 'Legacy Manual Value', 'investment', 'manual_investment', 'manual_value', 'CNY', NULL, NULL, 1, 1, 0, NULL, NULL, 1, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', '11111111-1111-4111-8111-111111111111', NULL, NULL, 'Legacy Liability', 'liability', 'personal_debt', 'balance', 'CNY', NULL, NULL, 1, 0, 0, NULL, NULL, 2, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('cccccccc-cccc-4ccc-8ccc-cccccccccccc', '11111111-1111-4111-8111-111111111111', '66666666-6666-4666-8666-666666666666', NULL, 'Archived Account', 'cash_equivalent', 'cash', 'balance', 'CNY', NULL, NULL, 1, 0, 1, NULL, NULL, 3, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z');

INSERT INTO account_ownership (account_id, member_id, share_bps)
VALUES
  ('99999999-9999-4999-8999-999999999999', '33333333-3333-4333-8333-333333333333', 10000),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', '33333333-3333-4333-8333-333333333333', 10000),
  ('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', '44444444-4444-4444-8444-444444444444', 10000),
  ('cccccccc-cccc-4ccc-8ccc-cccccccccccc', '44444444-4444-4444-8444-444444444444', 10000);

INSERT INTO account_values (id, account_id, value_kind, amount, currency, effective_at, created_at)
VALUES
  ('dddddddd-dddd-4ddd-8ddd-dddddddddddd', '99999999-9999-4999-8999-999999999999', 'balance', '90000', 'CNY', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
  ('eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee', '99999999-9999-4999-8999-999999999999', 'balance', '100000', 'CNY', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('ffffffff-ffff-4fff-8fff-ffffffffffff', 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', 'manual_value', '20000', 'CNY', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
  ('10101010-1010-4010-8010-101010101010', 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', 'manual_value', '25000', 'CNY', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('12121212-1212-4212-8212-121212121212', 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', 'balance', '5000', 'CNY', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('13131313-1313-4313-8313-131313131313', 'cccccccc-cccc-4ccc-8ccc-cccccccccccc', 'balance', '7000', 'CNY', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');

INSERT INTO app_settings (id, language, appearance, last_household_id, created_at, updated_at)
VALUES (1, 'en', 'light', '11111111-1111-4111-8111-111111111111', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
