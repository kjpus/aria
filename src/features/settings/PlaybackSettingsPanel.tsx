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
          ? 'Aria will try a WASAPI-exclusive output stream on Windows for the selected device and prefer track-native PCM layouts first. If the device rejects every compatible exclusive PCM layout, playback will fail instead of silently falling back to shared mode.'
          : 'Exclusive WASAPI is only available for Windows output endpoints that Aria can address directly.'}
      </p>
    </SectionCard>
  );
}
