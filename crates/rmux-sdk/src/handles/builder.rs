//! Builder for the opaque RMUX SDK facade.

use std::fmt;
use std::path::PathBuf;

use super::rmux::Rmux;
use crate::RmuxEndpoint;

/// Builder for an inert [`Rmux`] facade handle.
///
/// The builder only stores configuration. It does not resolve default
/// endpoints, touch the filesystem, open IPC handles, or require a running
/// daemon.
pub struct RmuxBuilder {
    endpoint: RmuxEndpoint,
}

impl RmuxBuilder {
    /// Creates a builder configured to use the platform default endpoint.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the daemon endpoint selection explicitly.
    ///
    /// Passing [`RmuxEndpoint::Default`] restores deferred platform-default
    /// endpoint resolution.
    #[must_use]
    pub fn endpoint(mut self, endpoint: RmuxEndpoint) -> Self {
        self.endpoint = endpoint;
        self
    }

    /// Sets an explicit Unix-domain socket path.
    ///
    /// This method is available on every compile target and only records the
    /// path; it does not require Unix at runtime.
    #[must_use]
    pub fn unix_socket(self, path: impl Into<PathBuf>) -> Self {
        self.endpoint(RmuxEndpoint::UnixSocket(path.into()))
    }

    /// Sets an explicit Windows named-pipe identifier.
    ///
    /// This method is available on every compile target and only records the
    /// pipe name; it does not require Windows at runtime.
    #[must_use]
    pub fn windows_pipe(self, pipe: impl Into<String>) -> Self {
        self.endpoint(RmuxEndpoint::WindowsPipe(pipe.into()))
    }

    /// Restores deferred platform-default endpoint resolution.
    #[must_use]
    pub fn default_endpoint(self) -> Self {
        self.endpoint(RmuxEndpoint::Default)
    }

    /// Returns the endpoint selection currently recorded by this builder.
    #[must_use]
    pub fn configured_endpoint(&self) -> &RmuxEndpoint {
        &self.endpoint
    }

    /// Builds an inert facade handle from the recorded configuration.
    ///
    /// Building does not contact the daemon or perform endpoint resolution.
    #[must_use]
    pub fn build(self) -> Rmux {
        Rmux::from_endpoint(self.endpoint)
    }
}

impl Default for RmuxBuilder {
    fn default() -> Self {
        Self {
            endpoint: RmuxEndpoint::Default,
        }
    }
}

impl fmt::Debug for RmuxBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RmuxBuilder")
            .finish_non_exhaustive()
    }
}
