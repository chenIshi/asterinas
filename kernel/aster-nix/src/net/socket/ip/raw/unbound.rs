// SPDX-License-Identifier: MPL-2.0

use alloc::sync::Weak;

use smoltcp::{socket::icmp, wire::IpListenEndpoint};

use super::bound::BoundRaw;
use crate::{
    events::{IoEvents, Observer},
    net::{
        iface::{AnyUnboundSocket, IpEndpoint, RawIcmpSocket},
        socket::ip::common::bind_socket,
    },
    prelude::*,
    process::signal::Pollee,
};

pub struct UnboundRaw {
    unbound_socket: Box<AnyUnboundSocket>,
}

impl UnboundRaw {
    pub fn new(observer: Weak<dyn Observer<()>>) -> Self {
        Self {
            unbound_socket: Box::new(AnyUnboundSocket::new_icmp(observer)),
        }
    }

    pub fn bind(self, endpoint: &IpEndpoint) -> core::result::Result<BoundRaw, (Error, Self)> {
        let bound_socket = match bind_socket(self.unbound_socket, endpoint, false) {
            Ok(bound_socket) => bound_socket,
            Err((err, unbound_socket)) => return Err((err, Self { unbound_socket })),
        };

        let bound_endpoint = bound_socket.local_endpoint().unwrap();
        bound_socket.raw_with(|socket: &mut RawIcmpSocket| {
            socket
                .bind(icmp::Endpoint::Udp(IpListenEndpoint::from(bound_endpoint)))
                .unwrap();
        });

        Ok(BoundRaw::new(bound_socket))
    }

    pub(super) fn init_pollee(&self, pollee: &Pollee) {
        pollee.reset_events();
        pollee.add_events(IoEvents::OUT);
    }
}
