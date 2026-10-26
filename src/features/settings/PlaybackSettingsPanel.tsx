import { SectionCard } from '../../components/SectionCard';
import type {
  OutputDeviceSnapshot,
  PlaybackPreferences,
} from '../../types/aria';

type PlaybackSettingsPanelProps = {
  playback: PlaybackPreferences;
  outputDevices: OutputDeviceSnapshot[];
  currentOutputDevice: OutputDeviceSnapshot;
  onChange: (playback: PlaybackPreferences) => void;
};

export function PlaybackSettingsPanel({
  playback,
  outputDevices,
  currentOutputDevice,
  onChange,
}: PlaybackSettingsPanelProps) {
  const selectedDevice =
    outputDevices.find((device) => device.id === playback.outputDeviceId) ?? null;
  const effectiveDevice = selectedDevice ?? currentOutputDevice;
  const exclusiveSupported = effectiveDevice.exclusiveCapable;

  return (
    <SectionCard eyebrow="Audio" title="Playback options">
      <div className="field-stack">
        <label className="field-label" htmlFor="output-device-select">
          Output device
        </label>
        <select
          id="output-device-select"
          value={playback.outputDeviceId ?? ''}
          onChange={(event) => {
            const nextOutputDeviceId = event.target.value || null;
            const nextDevice =
              outputDevices.find((device) => device.id === nextOutputDeviceId) ??
              (nextOutputDeviceId ? null : currentOutputDevice);

            onChange({
              ...playback,
              outputDeviceId: nextOutputDeviceId,
              exclusiveMode: nextDevice?.exclusiveCapable
                ? playback.exclusiveMode
                : false,
            });
          }}
        >
          <option value="">System default</option>
          {outputDevices.map((device) => (
            <option key={device.id} value={device.id}>
              {device.name}
              {device.isUsb ? ' (USB DAC)' : ''}
              {device.isBluetooth ? ' (Bluetooth HD)' : ''}
              {device.isDefault ? ' (default)' : ''}
            </option>
          ))}
        </select>
      </div>

      <div className="audio-device-summary">
        <div>
          <span>Current output</span>
          <strong>{currentOutputDevice.name}</strong>
        </div>
        <small>{currentOutputDevice.backend}</small>
      </div>

      <label className="checkbox-row" htmlFor="exclusive-mode-checkbox">
        <input
          checked={playback.exclusiveMode}
          disabled={!exclusiveSupported}
          id="exclusive-mode-checkbox"
          onChange={(event) =>
            onChange({
              ...playback,
              exclusiveMode: event.target.checked,
            })
          }
          type="checkbox"
        />
        <span>Exclusive mode / bit-accurate path</span>
      </label>

      <p className="dialog-section__note">
        {exclusiveSupported
          ? 'Aria will use a dedicated bit-accurate path for the selected device (WASAPI exclusive on Windows, direct bit-perfect routing for USB DACs, or direct HD audio routing for Bluetooth on Android) and prefer track-native PCM layouts first. If exclusive playback cannot be opened, Aria will not fall back to shared mode while this setting is enabled. The player-strip volume control is software attenuation, so leave it at 100% if you want an unchanged digital stream.'
          : 'Exclusive mode / bit-accurate path is available for supported endpoints that Aria can address directly (such as external USB DACs or Bluetooth HD headsets on Android, or WASAPI devices on Windows).'}
      </p>
    </SectionCard>
  );
}
