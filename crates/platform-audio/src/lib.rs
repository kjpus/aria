#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformAudioBackend {
    Wasapi,
    Asio,
    CoreAudio,
    Alsa,
    Pipewire,
    Dummy,
}

pub fn desktop_backends() -> Vec<PlatformAudioBackend> {
    vec![
        PlatformAudioBackend::Wasapi,
        PlatformAudioBackend::Asio,
        PlatformAudioBackend::CoreAudio,
        PlatformAudioBackend::Alsa,
        PlatformAudioBackend::Pipewire,
    ]
}
