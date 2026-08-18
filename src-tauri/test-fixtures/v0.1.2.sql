-- Synthetic released-schema fixture; no user data.
INSERT INTO households (id, name, base_currency, created_at, updated_at)
VALUES ('11111111-1111-4111-8111-111111111111', 'Fixture Household', 'CNY', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');

INSERT INTO media_assets (id, household_id, mime_type, data, created_at)
VALUES ('22222222-2222-4222-8222-222222222222', '11111111-1111-4111-8111-111111111111', 'image/png', X'89504E470D0A1A0A', '2026-01-01T00:00:00.000Z');

INSERT INTO members (id, household_id, name, avatar_asset_id, note, sort_order, created_at, updated_at, archived_at)
VALUES
  ('33333333-3333-4333-8333-333333333333', '11111111-1111-4111-8111-111111111111', 'Active Member', '22222222-2222-4222-8222-222222222222', 'fixture member', 0, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('34343434-3434-4343-8343-343434343434', '11111111-1111-4111-8111-111111111111', 'Second Member', NULL, NULL, 1, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('44444444-4444-4444-8444-444444444444', '11111111-1111-4111-8111-111111111111', 'Archived Member', NULL, NULL, 2, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z');

INSERT INTO institutions (id, household_id, name, institution_type, country_code, website, note, logo_asset_id, sort_order, created_at, updated_at, archived_at)
VALUES
  ('55555555-5555-4555-8555-555555555555', '11111111-1111-4111-8111-111111111111', 'Archived Bank', 'bank', 'CN', NULL, NULL, NULL, 0, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z'),
  ('66666666-6666-4666-8666-666666666666', '11111111-1111-4111-8111-111111111111', 'Active Broker', 'broker', 'SG', NULL, NULL, NULL, 1, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL);

INSERT INTO account_groups (id, household_id, name, icon_key, color, logo_asset_id, description, sort_order, created_at, updated_at, archived_at)
VALUES
  ('77777777-7777-4777-8777-777777777777', '11111111-1111-4111-8111-111111111111', 'Archived Group', NULL, NULL, NULL, NULL, 0, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z'),
  ('88888888-8888-4888-8888-888888888888', '11111111-1111-4111-8111-111111111111', 'Active Group', NULL, NULL, NULL, NULL, 1, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL);

INSERT INTO accounts (id, household_id, institution_id, group_id, name, primary_category, secondary_category, tracking_mode, default_currency, note, logo_asset_id, include_in_net_worth, include_in_investment, include_in_liquid_assets, opened_on, closed_on, sort_order, created_at, updated_at, archived_at)
VALUES
  ('99999999-9999-4999-8999-999999999999', '11111111-1111-4111-8111-111111111111', '66666666-6666-4666-8666-666666666666', '88888888-8888-4888-8888-888888888888', 'Brokerage', 'investment', 'brokerage_account', 'holdings', 'SGD', 'golden holdings 62190 CNY', NULL, 1, 1, 0, NULL, NULL, 0, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', '11111111-1111-4111-8111-111111111111', NULL, '88888888-8888-4888-8888-888888888888', 'Manual Investment', 'investment', 'manual_investment', 'manual_value', 'CNY', NULL, NULL, 1, 1, 0, NULL, NULL, 1, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', '11111111-1111-4111-8111-111111111111', '55555555-5555-4555-8555-555555555555', '77777777-7777-4777-8777-777777777777', 'Operating Cash', 'cash_equivalent', 'bank_account', 'balance', 'CNY', 'retained archived references', NULL, 1, 0, 1, NULL, NULL, 2, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('cccccccc-cccc-4ccc-8ccc-cccccccccccc', '11111111-1111-4111-8111-111111111111', '55555555-5555-4555-8555-555555555555', NULL, 'Archived Account', 'cash_equivalent', 'cash', 'balance', 'CNY', NULL, NULL, 1, 0, 1, NULL, NULL, 3, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z'),
  ('dddddddd-dddd-4ddd-8ddd-dddddddddddd', '11111111-1111-4111-8111-111111111111', NULL, NULL, 'Archived Liability', 'liability', 'personal_debt', 'balance', 'CNY', NULL, NULL, 1, 0, 0, NULL, NULL, 4, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z');

INSERT INTO account_ownership (account_id, member_id, share_bps)
VALUES
  ('99999999-9999-4999-8999-999999999999', '33333333-3333-4333-8333-333333333333', 6000),
  ('99999999-9999-4999-8999-999999999999', '34343434-3434-4343-8343-343434343434', 4000),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', '33333333-3333-4333-8333-333333333333', 10000),
  ('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', '34343434-3434-4343-8343-343434343434', 10000),
  ('cccccccc-cccc-4ccc-8ccc-cccccccccccc', '44444444-4444-4444-8444-444444444444', 10000),
  ('dddddddd-dddd-4ddd-8ddd-dddddddddddd', '44444444-4444-4444-8444-444444444444', 10000);

INSERT INTO account_values (id, account_id, value_kind, amount, currency, effective_at, created_at)
VALUES
  ('10101010-1010-4010-8010-101010101010', 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', 'manual_value', '800', 'CNY', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
  ('12121212-1212-4212-8212-121212121212', 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', 'manual_value', '1000', 'CNY', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('13131313-1313-4313-8313-131313131313', 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', 'balance', '50', 'CNY', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
  ('14141414-1414-4414-8414-141414141414', 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', 'balance', '0', 'CNY', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('15151515-1515-4515-8515-151515151515', 'cccccccc-cccc-4ccc-8ccc-cccccccccccc', 'balance', '5000', 'CNY', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
  ('16161616-1616-4616-8616-161616161616', 'cccccccc-cccc-4ccc-8ccc-cccccccccccc', 'balance', '7000', 'CNY', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('17171717-1717-4717-8717-171717171717', 'dddddddd-dddd-4ddd-8ddd-dddddddddddd', 'balance', '5000', 'CNY', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');

INSERT INTO instruments (id, household_id, name, symbol, instrument_type, quote_currency, market_code, country_code, isin, provider_key, provider_symbol, quote_preference, note, logo_asset_id, sort_order, created_at, updated_at, archived_at)
VALUES
  ('20202020-2020-4202-8202-202020202020', '11111111-1111-4111-8111-111111111111', 'Fixture QQQ', 'QQQ', 'etf', 'USD', 'XNAS', 'US', NULL, NULL, NULL, 'manual', NULL, NULL, 0, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('21212121-2121-4212-8212-212121212121', '11111111-1111-4111-8111-111111111111', 'Fixture ES3', 'ES3', 'etf', 'SGD', 'XSES', 'SG', NULL, NULL, NULL, 'manual', NULL, NULL, 1, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('23232323-2323-4323-8323-232323232323', '11111111-1111-4111-8111-111111111111', 'Archived Instrument', 'ARCH', 'stock', 'USD', NULL, 'US', NULL, NULL, NULL, 'manual', NULL, NULL, 2, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z'),
  ('24242424-2424-4424-8424-242424242424', '11111111-1111-4111-8111-111111111111', 'Fixture Index', 'FIX', 'etf', 'USD', NULL, 'US', NULL, 'fake', 'FIX', 'provider', NULL, NULL, 3, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL);

INSERT INTO holdings (id, account_id, instrument_id, quantity, note, sort_order, created_at, updated_at, archived_at)
VALUES
  ('30303030-3030-4303-8303-303030303030', '99999999-9999-4999-8999-999999999999', '20202020-2020-4202-8202-202020202020', '3', NULL, 0, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('31313131-3131-4313-8313-313131313131', '99999999-9999-4999-8999-999999999999', '21212121-2121-4212-8212-212121212121', '1000', NULL, 1, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
  ('32323232-3232-4323-8323-323232323232', '99999999-9999-4999-8999-999999999999', '23232323-2323-4323-8323-232323232323', '10', 'archived holding retained', 2, '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-03T00:00:00.000Z');

INSERT INTO account_cash_values (id, account_id, amount, currency, effective_at, created_at)
VALUES
  ('40404040-4040-4404-8404-404040404040', '99999999-9999-4999-8999-999999999999', '4000', 'SGD', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
  ('41414141-4141-4414-8414-414141414141', '99999999-9999-4999-8999-999999999999', '5000', 'SGD', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');

INSERT INTO instrument_quotes (id, instrument_id, unit_price, quote_currency, source_kind, source_key, delayed, quoted_at, created_at)
VALUES
  ('50505050-5050-4505-8505-505050505050', '20202020-2020-4202-8202-202020202020', '680', 'USD', 'manual', 'manual', 0, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
  ('51515151-5151-4515-8515-515151515151', '20202020-2020-4202-8202-202020202020', '700', 'USD', 'manual', 'manual', 0, '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('52525252-5252-4525-8525-525252525252', '21212121-2121-4212-8212-212121212121', '3.5', 'SGD', 'manual', 'manual', 0, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
  ('53535353-5353-4535-8535-535353535353', '21212121-2121-4212-8212-212121212121', '4', 'SGD', 'manual', 'manual', 0, '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('54545454-5454-4545-8545-545454545454', '24242424-2424-4424-8424-242424242424', '100', 'USD', 'provider', 'fake', 0, '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');

INSERT INTO fx_quote_preferences (household_id, currency_a, currency_b, source_kind, created_at, updated_at)
VALUES
  ('11111111-1111-4111-8111-111111111111', 'CNY', 'USD', 'manual', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('11111111-1111-4111-8111-111111111111', 'CNY', 'SGD', 'manual', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('11111111-1111-4111-8111-111111111111', 'SGD', 'USD', 'provider', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');

INSERT INTO fx_quotes (id, household_id, base_currency, quote_currency, rate, source_kind, source_key, delayed, quoted_at, created_at)
VALUES
  ('60606060-6060-4606-8606-606060606060', '11111111-1111-4111-8111-111111111111', 'USD', 'CNY', '6.8', 'manual', 'manual', 0, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
  ('61616161-6161-4616-8616-616161616161', '11111111-1111-4111-8111-111111111111', 'USD', 'CNY', '6.9', 'manual', 'manual', 0, '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('62626262-6262-4626-8626-626262626262', '11111111-1111-4111-8111-111111111111', 'SGD', 'CNY', '5.2', 'manual', 'manual', 0, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
  ('63636363-6363-4636-8636-636363636363', '11111111-1111-4111-8111-111111111111', 'SGD', 'CNY', '5.3', 'manual', 'manual', 0, '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
  ('64646464-6464-4646-8646-646464646464', '11111111-1111-4111-8111-111111111111', 'USD', 'SGD', '1.3', 'provider', 'fake', 0, '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');

INSERT INTO app_settings (id, language, appearance, last_household_id, created_at, updated_at)
VALUES (1, 'en', 'light', '11111111-1111-4111-8111-111111111111', '2026-01-01T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
