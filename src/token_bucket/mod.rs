use log::{debug, error};
use tokio::time::Instant;
use tokio::time::{sleep, Duration};

// Represent a token bucket rate limiter
pub struct TokenBucket {
    // Max capacity of the token bucket
    capacity: u64,
    // Number of token retrieved every sec
    quantum: u64,
    // Number of available token
    available: u64,
    // Last time available token has been computed
    last: Instant,
}

impl TokenBucket {
    /// Returns a token bucket
    ///
    /// # Arguments
    ///
    /// * `capacity` - Max capacity of the bucket token and number of available token at startup
    /// * `quantum` - Number of token retrieved every sec
    ///
    /// # Examples
    ///
    /// ```
    /// use probes::token_bucket::TokenBucket;
    /// let mut token_bucket = TokenBucket::new(60, 1);
    /// ```
    pub fn new(capacity: u64, quantum: u64) -> TokenBucket {
        debug!(
            "Create token bucket with capacity {}, quantum {}",
            capacity, quantum
        );
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

    fn update_counter(&mut self, token: u64) {
        self.available -= token;
        self.last = Instant::now();
    }

    fn compute_wait_duration(&mut self, token: u64) -> Duration {
        let token_needed: u64 = token - self.available;
        let time_to_wait: f64 = token_needed as f64 / self.quantum as f64;
        debug!("Wait for {}s to get enough token", time_to_wait);
        Duration::from_secs_f64(time_to_wait)
    }

    /// Wait for the number of requested token in the bucket token
    ///
    /// If the bucket token has already enough token don't wait
    ///
    /// # Arguments
    ///
    /// * `token` - Number of token requested
    pub async fn wait_for(
        &mut self,
        token: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.capacity < token {
            error!(
                "Requested token is bigger than max capacity {} < {}",
                self.capacity, token
            );
            return Err(format!(
                "Number of requested token ({}) is greater than the capacity ({}) \
            of the token bucket",
                token, self.capacity
            )
            .into());
        }

        if token == 0 {
            return Ok(());
        }

        // Update number of available token from time elapsed since last time max by the capacity
        self.available = self.available_token_since(self.last.elapsed().as_secs());

        if self.available >= token {
            debug!(
                "There are already enough available token {} >= {}",
                self.available, token
            );
            self.update_counter(token);
            return Ok(());
        }

        sleep(self.compute_wait_duration(token)).await;

        // Reset available and last time
        self.available = 0;
        self.last = Instant::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::token_bucket::TokenBucket;

    #[test]
    fn available_token_since() {
        let mut token_bucket = TokenBucket::new(10, 1);
        // Max capacity
        assert_eq!(token_bucket.available_token_since(1), 10);

        // Add 2 * quantum
        token_bucket.available = 0;
        assert_eq!(token_bucket.available_token_since(2), 2);
    }

    #[test]
    fn update_counter() {
        let mut token_bucket = TokenBucket::new(10, 1);
        // Reduce availabel token by 5
        assert_eq!(token_bucket.available, 10);
        token_bucket.update_counter(5);
        assert_eq!(token_bucket.available, 5);
    }

    #[test]
    fn compute_wait_duration() {
        let mut token_bucket = TokenBucket::new(10, 1);
        token_bucket.available = 0;
        assert_eq!(
            token_bucket.compute_wait_duration(5),
            Duration::from_secs_f64(5.00)
        );
    }
}
