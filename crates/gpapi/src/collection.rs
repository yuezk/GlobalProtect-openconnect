use std::{
  io,
  time::{Duration, Instant},
};

/// Invocation control borrowed by collection; interruption must never be treated
/// as an unavailable optional product.
pub trait CollectionControl {
  fn check(&self) -> io::Result<()>;
}

impl<F: Fn() -> io::Result<()>> CollectionControl for F {
  fn check(&self) -> io::Result<()> {
    self()
  }
}

pub struct CollectionBudget {
  deadline: Instant,
}

impl CollectionBudget {
  pub fn new(timeout: Duration) -> Self {
    Self {
      deadline: Instant::now() + timeout,
    }
  }
}

impl CollectionControl for CollectionBudget {
  fn check(&self) -> io::Result<()> {
    if Instant::now() >= self.deadline {
      Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "Device information collection timed out",
      ))
    } else {
      Ok(())
    }
  }
}
