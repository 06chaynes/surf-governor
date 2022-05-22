//! A [surf] middleware that implements rate-limiting using [governor].
//! The majority of this has been copied from [tide-governor](https://github.com/ohmree/tide-governor)
//! # Example
//! ```no_run
//! use surf_governor::GovernorMiddleware;
//! use surf::{Client, Request, http::Method};
//! use url::Url;
//!
//! #[async_std::main]
//! async fn main() -> surf::Result<()> {
//!     let req = Request::new(Method::Get, Url::parse("https://example.api")?);
//!     // Construct Surf client with a governor
//!     let client = Client::new().with(GovernorMiddleware::per_second(30)?);
//!     let res = client.send(req).await?;
//!     Ok(())
//! }
//! ```
//! [surf]: https://github.com/http-rs/surf
//! [governor]: https://github.com/antifuchs/governor

// TODO: figure out how to add jitter support using `governor::Jitter`.
// TODO: add usage examples (both in the docs and in an examples directory).
// TODO: add unit tests.
use governor::{
    clock::{Clock, DefaultClock},
    state::keyed::DefaultKeyedStateStore,
    Quota, RateLimiter,
};
use http_types::{headers, Response, StatusCode};
use lazy_static::lazy_static;
use std::{convert::TryInto, error::Error, num::NonZeroU32, sync::Arc, time::Duration};
use surf::{middleware::Next, Client, Request, Result};

lazy_static! {
    static ref CLOCK: DefaultClock = DefaultClock::default();
}

/// Once the rate limit has been reached, the middleware will respond with
/// status code 429 (too many requests) and a `Retry-After` header with the amount
/// of time that needs to pass before another request will be allowed.
#[derive(Debug, Clone)]
pub struct GovernorMiddleware {
    limiter: Arc<RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>>,
}

impl GovernorMiddleware {
    /// Constructs a rate-limiting middleware from a [`Duration`] that allows one request in the given time interval.
    ///
    /// If the time interval is zero, returns `None`.
    #[must_use]
    pub fn with_period(duration: Duration) -> Option<Self> {
        Some(Self {
            limiter: Arc::new(RateLimiter::<String, _, _>::keyed(Quota::with_period(
                duration,
            )?)),
        })
    }

    /// Constructs a rate-limiting middleware that allows a specified number of requests every second.
    ///
    /// Returns an error if `times` can't be converted into a [`NonZeroU32`].
    pub fn per_second<T>(times: T) -> Result<Self>
    where
        T: TryInto<NonZeroU32>,
        T::Error: Error + Send + Sync + 'static,
    {
        Ok(Self {
            limiter: Arc::new(RateLimiter::<String, _, _>::keyed(Quota::per_second(
                times.try_into()?,
            ))),
        })
    }

    /// Constructs a rate-limiting middleware that allows a specified number of requests every minute.
    ///
    /// Returns an error if `times` can't be converted into a [`NonZeroU32`].
    pub fn per_minute<T>(times: T) -> Result<Self>
    where
        T: TryInto<NonZeroU32>,
        T::Error: Error + Send + Sync + 'static,
    {
        Ok(Self {
            limiter: Arc::new(RateLimiter::<String, _, _>::keyed(Quota::per_minute(
                times.try_into()?,
            ))),
        })
    }

    /// Constructs a rate-limiting middleware that allows a specified number of requests every hour.
    ///
    /// Returns an error if `times` can't be converted into a [`NonZeroU32`].
    pub fn per_hour<T>(times: T) -> Result<Self>
    where
        T: TryInto<NonZeroU32>,
        T::Error: Error + Send + Sync + 'static,
    {
        Ok(Self {
            limiter: Arc::new(RateLimiter::<String, _, _>::keyed(Quota::per_hour(
                times.try_into()?,
            ))),
        })
    }
}

#[surf::utils::async_trait]
impl surf::middleware::Middleware for GovernorMiddleware {
    async fn handle(
        &self,
        req: Request,
        client: Client,
        next: Next<'_>,
    ) -> std::result::Result<surf::Response, http_types::Error> {
        match self
            .limiter
            .check_key(&req.url().host_str().unwrap().to_string())
        {
            Ok(_) => Ok(next.run(req, client).await?),
            Err(negative) => {
                let wait_time = negative.wait_time_from(CLOCK.now());
                let mut res = Response::new(StatusCode::TooManyRequests);
                res.insert_header(headers::RETRY_AFTER, wait_time.as_secs().to_string());
                Ok(res.try_into()?)
            }
        }
    }
}
