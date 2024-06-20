// SPDX-License-Identifier: MPL-2.0

use core::sync::atomic::{AtomicBool, Ordering};

use takeable::Takeable;

use self::{bound::BoundRaw, unbound::UnboundRaw};
use super::{common::get_ephemeral_endpoint, UNSPECIFIED_LOCAL_ENDPOINT};
use crate::{
    events::{IoEvents, Observer},
    fs::{file_handle::FileLike, utils::StatusFlags},
    net::{
        iface::IpEndpoint,
        poll_ifaces,
        socket::{
            util::{send_recv_flags::SendRecvFlags, socket_addr::SocketAddr},
            Socket,
        },
    },
    prelude::*,
    process::signal::{Pollee, Poller},
};

mod bound;
mod unbound;

pub struct RawSocket {
    inner: RwLock<Takeable<Inner>>,
    is_nonblocking: AtomicBool,
    pollee: Pollee,
}

enum Inner {
    Unbound(UnboundRaw),
    Bound(BoundRaw),
}

impl Inner {
    fn bind(self, endpoint: &IpEndpoint) -> core::result::Result<BoundRaw, (Error, Self)> {
        let unbound_raw = match self {
            Inner::Unbound(unbound_raw) => unbound_raw,
            Inner::Bound(bound_raw) => {
                return Err((
                    Error::with_message(Errno::EINVAL, "the socket is already bound to an address"),
                    Inner::Bound(bound_raw),
                ));
            }
        };

        let bound_raw = match unbound_raw.bind(endpoint) {
            Ok(bound_raw) => bound_raw,
            Err((err, unbound_raw)) => return Err((err, Inner::Unbound(unbound_raw))),
        };
        Ok(bound_raw)
    }

    fn bind_to_ephemeral_endpoint(
        self,
        remote_endpoint: &IpEndpoint,
    ) -> core::result::Result<BoundRaw, (Error, Self)> {
        if let Inner::Bound(bound_raw) = self {
            return Ok(bound_raw);
        }

        let endpoint = get_ephemeral_endpoint(remote_endpoint);
        self.bind(&endpoint)
    }
}

impl RawSocket {
    pub fn new(nonblocking: bool) -> Arc<Self> {
        Arc::new_cyclic(|me| {
            let unbound_raw = UnboundRaw::new(me.clone() as _);
            let pollee = Pollee::new(IoEvents::empty());
            unbound_raw.init_pollee(&pollee);
            Self {
                inner: RwLock::new(Takeable::new(Inner::Unbound(unbound_raw))),
                is_nonblocking: AtomicBool::new(nonblocking),
                pollee,
            }
        })
    }

    pub fn is_nonblocking(&self) -> bool {
        self.is_nonblocking.load(Ordering::SeqCst)
    }

    pub fn set_nonblocking(&self, nonblocking: bool) {
        self.is_nonblocking.store(nonblocking, Ordering::SeqCst);
    }

    fn remote_endpoint(&self) -> Option<IpEndpoint> {
        let inner = self.inner.read();

        match inner.as_ref() {
            Inner::Bound(bound_raw) => bound_raw.remote_endpoint(),
            Inner::Unbound(_) => None,
        }
    }

    fn try_bind_empheral(&self, remote_endpoint: &IpEndpoint) -> Result<()> {
        // Fast path
        if let Inner::Bound(_) = self.inner.read().as_ref() {
            return Ok(());
        }

        // Slow path
        let mut inner = self.inner.write();
        inner.borrow_result(|owned_inner| {
            let bound_raw = match owned_inner.bind_to_ephemeral_endpoint(remote_endpoint) {
                Ok(bound_raw) => bound_raw,
                Err((err, err_inner)) => {
                    return (err_inner, Err(err));
                }
            };
            bound_raw.init_pollee(&self.pollee);
            (Inner::Bound(bound_raw), Ok(()))
        })
    }

    fn try_recvfrom(&self, buf: &mut [u8], flags: SendRecvFlags) -> Result<(usize, SocketAddr)> {
        let inner = self.inner.read();

        let Inner::Bound(bound_raw) = inner.as_ref() else {
            return_errno_with_message!(Errno::EAGAIN, "the socket is not bound");
        };

        let (recv_bytes, remote_endpoint) = bound_raw.try_recvfrom(buf, flags)?;
        bound_raw.update_io_events(&self.pollee);
        Ok((recv_bytes, remote_endpoint.into()))
    }

    fn try_sendto(&self, buf: &[u8], remote: &IpEndpoint, flags: SendRecvFlags) -> Result<usize> {
        let inner = self.inner.read();

        let Inner::Bound(bound_raw) = inner.as_ref() else {
            return_errno_with_message!(Errno::EAGAIN, "the socket is not bound")
        };

        let sent_bytes = bound_raw.try_sendto(buf, remote, flags)?;
        bound_raw.update_io_events(&self.pollee);
        Ok(sent_bytes)
    }

    // TODO: Support timeout
    fn wait_events<F, R>(&self, mask: IoEvents, mut cond: F) -> Result<R>
    where
        F: FnMut() -> Result<R>,
    {
        let poller = Poller::new();

        loop {
            match cond() {
                Err(err) if err.error() == Errno::EAGAIN => (),
                result => return result,
            };

            let events = self.poll(mask, Some(&poller));
            if !events.is_empty() {
                continue;
            }

            poller.wait()?;
        }
    }

    fn update_io_events(&self) {
        let inner = self.inner.read();
        let Inner::Bound(bound_raw) = inner.as_ref() else {
            return;
        };
        bound_raw.update_io_events(&self.pollee);
    }
}

impl FileLike for RawSocket {
    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        // FIXME: respect flags
        let flags = SendRecvFlags::empty();
        let (recv_len, _) = self.recvfrom(buf, flags)?;
        Ok(recv_len)
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        // FIXME: set correct flags
        let flags = SendRecvFlags::empty();
        self.sendto(buf, None, flags)
    }

    fn poll(&self, mask: IoEvents, poller: Option<&Poller>) -> IoEvents {
        self.pollee.poll(mask, poller)
    }

    fn as_socket(self: Arc<Self>) -> Option<Arc<dyn Socket>> {
        Some(self)
    }

    fn status_flags(&self) -> StatusFlags {
        if self.is_nonblocking() {
            StatusFlags::O_NONBLOCK
        } else {
            StatusFlags::empty()
        }
    }

    fn set_status_flags(&self, new_flags: StatusFlags) -> Result<()> {
        if new_flags.contains(StatusFlags::O_NONBLOCK) {
            self.set_nonblocking(true);
        } else {
            self.set_nonblocking(false);
        }
        Ok(())
    }

    fn register_observer(
        &self,
        observer: Weak<dyn Observer<IoEvents>>,
        mask: IoEvents,
    ) -> Result<()> {
        self.pollee.register_observer(observer, mask);
        Ok(())
    }

    fn unregister_observer(
        &self,
        observer: &Weak<dyn Observer<IoEvents>>,
    ) -> Option<Weak<dyn Observer<IoEvents>>> {
        self.pollee.unregister_observer(observer)
    }
}

impl Socket for RawSocket {
    fn bind(&self, socket_addr: SocketAddr) -> Result<()> {
        let endpoint = socket_addr.try_into()?;

        let mut inner = self.inner.write();
        inner.borrow_result(|owned_inner| {
            let bound_raw = match owned_inner.bind(&endpoint) {
                Ok(bound_raw) => bound_raw,
                Err((err, err_inner)) => {
                    return (err_inner, Err(err));
                }
            };
            bound_raw.init_pollee(&self.pollee);
            (Inner::Bound(bound_raw), Ok(()))
        })
    }

    fn addr(&self) -> Result<SocketAddr> {
        let inner = self.inner.read();
        match inner.as_ref() {
            Inner::Unbound(unbound_raw) => Ok(UNSPECIFIED_LOCAL_ENDPOINT.into()),
            Inner::Bound(bound_raw) => Ok(bound_raw.local_endpoint().into()),
        }
    }

    fn peer_addr(&self) -> Result<SocketAddr> {
        self.remote_endpoint()
            .map(|endpoint| endpoint.into())
            .ok_or_else(|| Error::with_message(Errno::ENOTCONN, "the socket is not connected"))
    }

    // FIXME: respect RecvFromFlags
    fn recvfrom(&self, buf: &mut [u8], flags: SendRecvFlags) -> Result<(usize, SocketAddr)> {
        debug_assert!(flags.is_all_supported());

        poll_ifaces();
        if self.is_nonblocking() {
            self.try_recvfrom(buf, flags)
        } else {
            self.wait_events(IoEvents::IN, || self.try_recvfrom(buf, flags))
        }
    }

    fn sendto(
        &self,
        buf: &[u8],
        remote: Option<SocketAddr>,
        flags: SendRecvFlags,
    ) -> Result<usize> {
        debug_assert!(flags.is_all_supported());

        let remote_endpoint = match remote {
            Some(remote_addr) => {
                let endpoint = remote_addr.try_into()?;
                self.try_bind_empheral(&endpoint)?;
                endpoint
            }
            None => self.remote_endpoint().ok_or_else(|| {
                Error::with_message(
                    Errno::EDESTADDRREQ,
                    "the destination address is not specified",
                )
            })?,
        };

        // TODO: Block if the send buffer is full
        let sent_bytes = self.try_sendto(buf, &remote_endpoint, flags)?;
        poll_ifaces();
        Ok(sent_bytes)
    }
}

impl Observer<()> for RawSocket {
    fn on_events(&self, events: &()) {
        self.update_io_events();
    }
}
