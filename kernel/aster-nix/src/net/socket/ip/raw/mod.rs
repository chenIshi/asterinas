// SPDX-License-Identifier: MPL-2.0

use core::sync::atomic::{AtomicBool};

use crate::{
    fs::{file_handle::FileLike},
    net::socket::Socket,
    prelude::*,
};

pub struct RawSocket {
    is_nonblocking: AtomicBool,
}

impl RawSocket {
    pub fn new(nonblocking: bool) -> Arc<Self> {
        todo!()
    }
}

impl FileLike for RawSocket {

}

impl Socket for RawSocket {

}