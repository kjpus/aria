create table if not exists remote_targets (
  id text primary key,
  name text not null,
  backend_type text not null,
  storage_limit_bytes integer,
  is_enabled integer not null default 1,
  config_json text not null default '{}'
);

create table if not exists remote_synced_items (
  item_id text not null,
  remote_target_id text not null,
  item_type text not null,
  remote_path text not null,
  remote_file_id text,
  size_bytes integer not null default 0,
  checksum text,
  sync_status text not null default 'synced',
  last_synced_at text not null,
  primary key (item_id, remote_target_id)
);

create index if not exists idx_remote_synced_items_target on remote_synced_items(remote_target_id);
