use tokio::time::Instant;
use tokio::time::{sleep, Duration};

pub struct TokenBucket {
    capacity: u64,
    quantum: u64,
    available: u64,
    last: Instant,
}


impl TokenBucket {
    pub fn new(capacity: u64, quantum: u64) -> TokenBucket {
        TokenBucket {
            capacity,
            quantum,
            available: capacity,
            last: Instant::now(),
        }
    }

    fn available_token_since(&mut self, elaspsed: u64) -> u64 {
        self.capacity.min(self.available + elaspsed * self.quantum)
    }

    fn update_counter(&mut self, count: u64) {
        self.available -= count;
        self.last = Instant::now();
    }

    async fn wait(&mut self, count: u64) {
        // Time to wait
        let token_needed: u64 = count - self.available;
        let time_to_wait: f64 = token_needed as f64 / self.quantum as f64;
        sleep(Duration::from_secs_f64(time_to_wait)).await;
    }

    pub async fn wait_for(&mut self, count: u64) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.capacity < count {
            return Err(format!("Number of requested token ({}) is greater than the capacity ({}) \
            of the token bucket", count, self.capacity).into());
        }

        if count == 0 {
            return Ok(());
        }

        // Update number of available token from time elapsed since last time max by the capacity
        self.available = self.available_token_since(self.last.elapsed().as_secs());

        if self.available >= count {
            self.update_counter(count);
            return Ok(());
        }

        self.wait(count).await;

        // Reset available and last time
        self.available = 0;
        self.last = Instant::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::token_bucket::TokenBucket;

    #[test]
    fn available_token_since_test() {
        let mut token_bucket = TokenBucket::new(10, 1);
        // Max capacity
        assert_eq!(token_bucket.available_token_since(1), 10);

        // Add 2 * quantum
        token_bucket.available = 0;
        assert_eq!(token_bucket.available_token_since(2), 2);
    }
}