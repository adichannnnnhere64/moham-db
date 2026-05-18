-- Seed LOCAL (destination) with fewer rows than REMOTE so you can test syncing.

INSERT INTO customers (id, external_ref, name, email, created_at, updated_at) VALUES
  (1, 'CUST-0001', 'Alice Local', 'alice@example.com', '2026-05-01 10:00:00', '2026-05-01 10:00:00'),
  (2, 'CUST-0002', 'Bob Local',   'bob@example.com',   '2026-05-01 10:05:00', '2026-05-01 10:05:00');

INSERT INTO products (id, sku, name, price_cents, created_at, updated_at) VALUES
  (1, 'SKU-RED-TSHIRT', 'Red T-Shirt', 1999, '2026-05-01 11:00:00', '2026-05-01 11:00:00'),
  (2, 'SKU-BLUE-HAT',   'Blue Hat',    1299, '2026-05-01 11:01:00', '2026-05-01 11:01:00');

INSERT INTO orders (id, order_number, customer_id, status, total_cents, ordered_at, created_at, updated_at) VALUES
  (1, 'ORD-LOCAL-0001', 1, 'paid', 3298, '2026-05-02 09:00:00', '2026-05-02 09:00:00', '2026-05-02 09:00:00');

INSERT INTO order_items (id, order_id, product_id, quantity, unit_price_cents, created_at, updated_at) VALUES
  (1, 1, 1, 1, 1999, '2026-05-02 09:00:00', '2026-05-02 09:00:00'),
  (2, 1, 2, 1, 1299, '2026-05-02 09:00:00', '2026-05-02 09:00:00');

