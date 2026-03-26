# L2CAP on Windows — Research Notes

## Summary

L2CAP (Logical Link Control and Adaptation Protocol) is required for the AAP (Apple
Accessory Protocol) channel to AirPods on PSM 0x1001. This is a **classic Bluetooth
(BR/EDR)** L2CAP connection, not BLE.

## Approach 1: Winsock AF_BTH + BTHPROTO_L2CAP (chosen)

Windows Winsock supports Bluetooth sockets via `AF_BTH` address family. The supported
socket triples include:

- `AF_BTH, SOCK_STREAM, BTHPROTO_L2CAP`
- `AF_BTH, SOCK_STREAM, 0`
- `AF_BTH, 0, BTHPROTO_L2CAP`

The `SOCKADDR_BTH` structure has a `port` field that accepts an L2CAP PSM value.
This is accessible from **user mode** without any kernel driver.

The `windows` Rust crate (`windows::Win32::Devices::Bluetooth`) provides:
- `SOL_L2CAP` constant
- `BTHPROTO_L2CAP` constant
- `SOCKADDR_BTH` structure
- `BTH_ADDR` type

And `windows::Win32::Networking::WinSock` provides the standard socket functions
(`socket`, `connect`, `send`, `recv`, `closesocket`).

**Status**: Implemented in `src/l2cap.rs`. Uses `spawn_blocking` for synchronous
Winsock calls within a tokio async context. Compiles on Linux with `#[cfg(target_os = "windows")]` guards (stub on non-Windows).

**Known limitations**:
- Winsock L2CAP uses `SOCK_STREAM` which provides a stream-oriented interface.
  The Linux daemon uses `SeqPacket` (message-oriented). AAP packets are small and
  self-framing, so stream should work, but may need length-prefixed framing if
  packet boundaries are lost.
- `SOCKADDR_BTH.port` for L2CAP PSM may require the PSM to be registered in the
  system's SDP database, or it may work for arbitrary PSMs. Testing required.
- May require the AirPods to be paired (bonded) in Windows Bluetooth settings first.

## Approach 2: KMDF L2CAP Bridge Driver (alternative)

The [WinPods project](https://github.com/changcheng967/WinPods) uses a KMDF
(Kernel-Mode Driver Framework) driver that:
1. Registers as a Bluetooth profile driver
2. Opens an L2CAP channel using BRB_L2CA_OPEN_CHANNEL
3. Exposes the channel to user mode via a custom IOCTL interface

**Pros**: Full control over L2CAP parameters, no SDP registration needed
**Cons**: Requires driver development (C/C++ + WDK), test signing mode, manual install

This is the fallback if Winsock L2CAP proves insufficient.

## Approach 3: WinRT Bluetooth APIs (not suitable)

`Windows.Devices.Bluetooth` WinRT APIs provide:
- BLE GATT operations (not what we need)
- RFCOMM (serial port profile, wrong protocol)
- No direct L2CAP PSM connection API

The WinRT APIs do not expose raw L2CAP channel creation for classic Bluetooth.

## BLE Discovery

BLE scanning works fine cross-platform via `btleplug`, which uses WinRT
`Windows.Devices.Bluetooth.Advertisement` APIs on Windows. The BLE scan finds the
AirPods by service UUID or Apple manufacturer data. After discovery, the BT address
is extracted and used for the L2CAP connection (which is classic BT, not BLE).

## Key References

- [Microsoft: Creating a L2CAP Client Connection](https://learn.microsoft.com/en-us/windows-hardware/drivers/bluetooth/creating-a-l2cap-client-connection-to-a-remote-device)
- [Microsoft: Bluetooth Echo L2CAP Profile Driver](https://learn.microsoft.com/en-us/samples/microsoft/windows-driver-samples/bluetooth-echo-l2cap-profile-driver/)
- [OSR Forum: L2CAP in User Mode](https://community.osr.com/t/is-it-possible-to-expose-microsoft-bluetooth-stack-l2cap-functionality-to-user-mode/49120)
- [windows-rs Bluetooth API docs](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Devices/Bluetooth/index.html)
- [WinPods (KMDF driver approach)](https://github.com/changcheng967/WinPods)
- [btleplug (BLE scanning)](https://github.com/deviceplug/btleplug)
