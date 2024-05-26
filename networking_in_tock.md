# Networking in Tock

## Layers:

`struct RadioDriver` - IEEE 802.15.4 userspace interface for configuration and transmit/receive.
^
`struct MuxMac` - provides multiplexed access to an 802.15.4 MAC device.
^
`trait MacDevice` - the contract satisfied by an implementation of an IEEE 802.15.4 MAC device.
^
`trait Mac` - interface for IEEE 802.15.4 MAC protocol layers.
^
`struct Framer` - hides header preparation, transmission and processing logic from the user.
^
`trait Radio: RadioConfig + RadioData`
^
IEEE 802.15.4



### Framer
This struct wraps an IEEE 802.15.4 radio device `kernel::hil::radio::Radio`
and exposes IEEE 802.15.4 MAC device functionality as the trait
`capsules::mac::Mac`. It hides header preparation, transmission and
processing logic from the user by essentially maintaining multiple state
machines corresponding to the transmission, reception and
encryption/decryption pipelines. See the documentation in
`capsules/src/mac.rs` for more details.

### Mac
Specifies the interface for IEEE 802.15.4 MAC protocol layers. MAC protocols
expose similar configuration (address, PAN, transmission power) options
as ieee802154::device::MacDevice layers above it, but retain control over
radio power management and channel selection. All frame processing should
be completed above this layer such that Mac implementations receive fully
formatted 802.15.4 MAC frames for transmission.

### MacDevice
The contract satisfied by an implementation of an IEEE 802.15.4 MAC device.
Any IEEE 802.15.4 MAC device should expose the following high-level
functionality:
- Configuration of addresses and transmit power
- Preparing frames (data frame, command frames, beacon frames)
- Transmitting and receiving frames
Outlining this in a trait allows other implementations of MAC devices that
divide the responsibilities of software and hardware differently. For
example, a radio chip might be able to completely inline the frame security
procedure in hardware, as opposed to requiring a software implementation.

### MuxMac
`MuxMac` provides multiplexed access to an 802.15.4 MAC device. This enables
a single underlying 802.15.4 radio to be shared transparently by multiple
users. For example, the kernel might want to send raw 802.15.4 frames and
subsequently 6LoWPAN-encoded and fragmented IP packets. This capsule allows
that to happen by providing a mechanism for sequencing transmission attempts,
Every radio frame received is provided to all listening clients so that each
client can perform its own frame filtering logic.

### RadioDriver
IEEE 802.15.4 userspace interface for configuration and transmit/receive.

Implements a userspace interface for sending and receiving IEEE 802.15.4
frames. Also provides a minimal list-based interface for managing keys and
known link neighbors, which is needed for 802.15.4 security.

The driver functionality can be divided into three aspects: sending
packets, receiving packets, and managing the 15.4 state (i.e. keys, neighbors,
buffers, addressing, etc). The general design and procedure for sending and
receiving is discussed below.

Sending - The driver supports two modes of sending: Raw and Parse. In Raw mode,
the userprocess fully forms the 15.4 frame and passes it to the driver. In Parse
mode, the userprocess provides the payload and relevant metadata. From this
the driver forms the 15.4 header and secures the payload. To send a packet,
the userprocess issues the respective send command syscall (corresponding to
raw or parse mode of sending). The 15.4 capsule will then schedule an upcall,
upon completion of the transmission, to notify the process.

Receiving - The driver receives 15.4 frames and passes them to the userprocess.
To accomplish this, the userprocess must first `allow` a read/write ring buffer
to the kernel. The kernel will then fill this buffer with received frames and
schedule an upcall upon receipt of the first packet. When handling the upcall
the userprocess must first `unallow` the buffer as described in section 4.4 of
TRD104-syscalls. After unallowing the buffer, the userprocess must then immediately
clear all pending/scheduled receive upcalls. This is done by either unsubscribing
the receive upcall or subscribing a new receive upcall. Because the userprocess
provides the buffer, it is responsible for adhering to this procedure. Failure
to comply may result in dropped or malformed packets.

The ring buffer provided by the userprocess must be of the form:

```text
| read index | write index | user_frame 0 | user_frame 1 | ... | user_frame n |
```

`user_frame` denotes the 15.4 frame in addition to the relevant 3 bytes of
metadata (offset to data payload, length of data payload, and the MIC len). The
capsule assumes that this is the form of the buffer. Errors or deviation in
the form of the provided buffer will likely result in incomplete or dropped packets.

Because the scheduled receive upcall must be handled by the userprocess, there is
no guarantee as to when this will occur and if additional packets will be received
prior to the upcall being handled. Without a ring buffer (or some equivalent data
structure), the original packet will be lost. The ring buffer allows for the upcall
to be scheduled and for all received packets to be passed to the process. The ring
buffer is designed to overwrite old packets if the buffer becomes full. If the
userprocess notices a high number of "dropped" packets, this may be the cause. The
userproceess can mitigate this issue by increasing the size of the ring buffer
provided to the capsule.
