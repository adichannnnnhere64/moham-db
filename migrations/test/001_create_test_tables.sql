CREATE TABLE IF NOT EXISTS remote_orders (
  id INT NOT NULL AUTO_INCREMENT PRIMARY KEY,
  customer_name VARCHAR(255) NOT NULL,
  amount DECIMAL(10, 2) NOT NULL DEFAULT 0.00,
  status VARCHAR(50) NOT NULL DEFAULT 'pending',
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);

INSERT INTO remote_orders (customer_name, amount, status) VALUES
  ('Alice', 99.99, 'completed'),
  ('Bob', 149.50, 'pending'),
  ('Carol', 299.00, 'shipped');
