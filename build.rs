fn main() {
    windows::build!(
        Windows::Win32::Networking::WinSock::*,
        Windows::Win32::System::SystemServices::*,
        Windows::Win32::NetworkManagement::IpHelper::*,
    )
}
