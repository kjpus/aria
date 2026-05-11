pragma journal_mode = wal;

create table if not exists app_settings (
  setting_key text primary key,
  value_json text not null
);

create table if not exists library_state (
  snapshot_id integer primary key check (snapshot_id = 1),
  indexed_files integer not null default 0,
  last_scan_at text
);

create table if not exists library_roots (
  path text primary key,
  position integer not null,
  label text not null
);

create table if not exists field_mappings (
  position integer primary key,
  format text not null default 'DEFAULT',
  field_key text not null,
  label text not null,
  tag_priorities_json text not null
);

create table if not exists catalog_rules (
  position integer primary key,
  label text not null,
  pattern text not null,
  composers_json text not null,
  source_tags_json text not null,
  enabled integer not null default 1
);

create table if not exists tag_inventory (
  tag text primary key,
  occurrences integer not null,
  example_values_json text not null
);

create table if not exists scanned_tracks (
  id text primary key,
  path text not null,
  file_name text not null,
  album_art_path text,
  audio_json text not null,
  raw_tags_json text not null,
  mapped_fields_json text not null
);

create index if not exists idx_scanned_tracks_path on scanned_tracks(path);

insert or ignore into app_settings (setting_key, value_json)
values ('playback_session', '{"queue":[],"orderedQueue":[],"currentQueueIndex":null}');
