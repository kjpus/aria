import type { LibraryFieldMapping } from '../types/aria';

export type FieldMappingFormat =
  | 'DEFAULT'
  | 'FLAC'
  | 'MP3'
  | 'MP4'
  | 'AAC'
  | 'OGG'
  | 'OPUS'
  | 'WAV'
  | 'AIFF';

export const DEFAULT_FIELD_MAPPING_FORMAT: FieldMappingFormat = 'FLAC';

export const FIELD_MAPPING_FORMAT_OPTIONS: Array<{
  label: string;
  value: FieldMappingFormat;
}> = [
  { label: 'FLAC', value: 'FLAC' },
  { label: 'MP3', value: 'MP3' },
  { label: 'MP4 / M4A', value: 'MP4' },
  { label: 'AAC', value: 'AAC' },
  { label: 'Ogg Vorbis', value: 'OGG' },
  { label: 'Opus', value: 'OPUS' },
  { label: 'WAV', value: 'WAV' },
  { label: 'AIFF / AIF', value: 'AIFF' },
  { label: 'Default fallback', value: 'DEFAULT' },
];

export function normalizeFieldMappingFormat(format: string): FieldMappingFormat {
  const normalized = format.trim().toUpperCase();

  switch (normalized) {
    case '':
      return 'DEFAULT';
    case 'M4A':
    case 'MP4':
      return 'MP4';
    case 'AIF':
    case 'AIFF':
      return 'AIFF';
    case 'AAC':
      return 'AAC';
    case 'FLAC':
      return 'FLAC';
    case 'MP3':
      return 'MP3';
    case 'OGG':
      return 'OGG';
    case 'OPUS':
      return 'OPUS';
    case 'WAV':
      return 'WAV';
    case 'DEFAULT':
      return 'DEFAULT';
    default:
      return (normalized || 'DEFAULT') as FieldMappingFormat;
  }
}

export function fieldMappingFormatLabel(format: string): string {
  const normalized = normalizeFieldMappingFormat(format);
  return (
    FIELD_MAPPING_FORMAT_OPTIONS.find((option) => option.value === normalized)?.label ??
    normalized
  );
}

export function selectInitialFieldMappingFormat(mappings: LibraryFieldMapping[]): FieldMappingFormat {
  const availableFormats = new Set(
    mappings.map((mapping) => normalizeFieldMappingFormat(mapping.format)),
  );

  if (availableFormats.has(DEFAULT_FIELD_MAPPING_FORMAT)) {
    return DEFAULT_FIELD_MAPPING_FORMAT;
  }

  const selected =
    mappings.find((mapping) => normalizeFieldMappingFormat(mapping.format) !== 'DEFAULT')
      ?.format ?? mappings[0]?.format ?? 'DEFAULT';

  return normalizeFieldMappingFormat(selected);
}

export function defaultFieldMappings(): LibraryFieldMapping[] {
  const defaults: FieldMappingFormat[] = [
    'DEFAULT',
    'FLAC',
    'MP3',
    'MP4',
    'AAC',
    'OGG',
    'OPUS',
    'WAV',
    'AIFF',
  ];

  return defaults.flatMap((format) =>
    defaultFieldMappingsForFormat(format),
  );
}

export function defaultFieldMappingsForFormat(format: string): LibraryFieldMapping[] {
  const profile = defaultMappingProfile(format);

  return [
    createFieldMapping(format, 'album', 'Album', profile.album),
    createFieldMapping(format, 'title', 'Title', profile.title),
    createFieldMapping(format, 'catalog', 'Catalog', profile.catalog),
    createFieldMapping(format, 'composer', 'Composer', profile.composer),
    createFieldMapping(format, 'genre', 'Genre', profile.genre),
    createFieldMapping(format, 'conductor', 'Conductor', profile.conductor),
    createFieldMapping(format, 'ensemble', 'Ensemble', profile.ensemble),
    createFieldMapping(format, 'soloist', 'Soloist', profile.soloist),
    createFieldMapping(format, 'year', 'Year', profile.year),
    createFieldMapping(format, 'disk_number', 'Disk Number', profile.diskNumber),
    createFieldMapping(format, 'track_number', 'Track Number', profile.trackNumber),
  ];
}

type DefaultMappingProfile = {
  album: string[];
  title: string[];
  catalog: string[];
  composer: string[];
  genre: string[];
  conductor: string[];
  ensemble: string[];
  soloist: string[];
  year: string[];
  diskNumber: string[];
  trackNumber: string[];
};

function defaultMappingProfile(format: string): DefaultMappingProfile {
  switch (normalizeFieldMappingFormat(format)) {
    case 'DEFAULT':
      return defaultFallbackMappingProfile();
    case 'FLAC':
      return flacMappingProfile();
    case 'MP3':
      return mp3MappingProfile();
    case 'MP4':
      return mp4MappingProfile();
    case 'AAC':
      return aacMappingProfile();
    case 'OGG':
      return oggMappingProfile();
    case 'OPUS':
      return opusMappingProfile();
    case 'WAV':
      return wavMappingProfile();
    case 'AIFF':
      return aiffMappingProfile();
    default:
      return defaultFallbackMappingProfile();
  }
}

function defaultFallbackMappingProfile(): DefaultMappingProfile {
  return {
    album: ['ALBUM'],
    title: ['TITLE'],
    catalog: ['CATALOGNUMBER', 'CATALOG'],
    composer: ['COMPOSER', 'WORKCOMPOSER', 'COMPOSERSORT'],
    genre: ['GENRE'],
    conductor: ['CONDUCTOR'],
    ensemble: ['ENSEMBLE', 'ORCHESTRA', 'ALBUMARTIST'],
    soloist: ['SOLOIST', 'PERFORMER', 'ARTIST', 'ALBUMARTIST'],
    year: ['DATE', 'YEAR'],
    diskNumber: ['DISCNUMBER', 'DISKNUMBER', 'DISC'],
    trackNumber: ['TRACKNUMBER', 'TRACK'],
  };
}

function flacMappingProfile(): DefaultMappingProfile {
  return {
    album: ['ALBUM'],
    title: ['TITLE'],
    catalog: ['CATALOGNUMBER', 'CATALOG'],
    composer: ['COMPOSER', 'WORKCOMPOSER', 'COMPOSERSORT'],
    genre: ['GENRE'],
    conductor: ['CONDUCTOR'],
    ensemble: ['ENSEMBLE', 'ORCHESTRA', 'ALBUMARTIST', 'ARTIST'],
    soloist: ['SOLOIST', 'PERFORMER', 'ARTIST', 'ALBUMARTIST'],
    year: ['DATE', 'YEAR'],
    diskNumber: ['DISCNUMBER', 'DISKNUMBER', 'DISC'],
    trackNumber: ['TRACKNUMBER', 'TRACK'],
  };
}

function mp3MappingProfile(): DefaultMappingProfile {
  return {
    album: ['ALBUM'],
    title: ['TITLE'],
    catalog: ['CATALOGNUMBER', 'CATALOG'],
    composer: ['COMPOSER', 'WORKCOMPOSER', 'COMPOSERSORT'],
    genre: ['GENRE'],
    conductor: ['CONDUCTOR'],
    ensemble: ['BAND', 'ORCHESTRA', 'ENSEMBLE', 'ALBUMARTIST'],
    soloist: ['SOLOIST', 'ARTIST', 'PERFORMER', 'ALBUMARTIST'],
    year: ['DATE', 'YEAR'],
    diskNumber: ['DISCNUMBER', 'DISKNUMBER', 'DISC'],
    trackNumber: ['TRACKNUMBER', 'TRACK'],
  };
}

function aacMappingProfile(): DefaultMappingProfile {
  return {
    album: ['ALBUM'],
    title: ['TITLE'],
    catalog: ['CATALOGNUMBER', 'CATALOG'],
    composer: ['COMPOSER', 'WORKCOMPOSER', 'COMPOSERSORT'],
    genre: ['GENRE'],
    conductor: ['CONDUCTOR'],
    ensemble: ['BAND', 'ORCHESTRA', 'ENSEMBLE', 'ALBUMARTIST'],
    soloist: ['SOLOIST', 'ARTIST', 'PERFORMER', 'ALBUMARTIST'],
    year: ['DATE', 'YEAR'],
    diskNumber: ['DISCNUMBER', 'DISKNUMBER', 'DISC'],
    trackNumber: ['TRACKNUMBER', 'TRACK'],
  };
}

function oggMappingProfile(): DefaultMappingProfile {
  return {
    album: ['ALBUM'],
    title: ['TITLE'],
    catalog: ['CATALOGNUMBER', 'CATALOG'],
    composer: ['COMPOSER', 'WORKCOMPOSER', 'COMPOSERSORT'],
    genre: ['GENRE'],
    conductor: ['CONDUCTOR'],
    ensemble: ['ENSEMBLE', 'ORCHESTRA', 'ALBUMARTIST', 'ARTIST'],
    soloist: ['SOLOIST', 'PERFORMER', 'ARTIST', 'ALBUMARTIST'],
    year: ['DATE', 'YEAR'],
    diskNumber: ['DISCNUMBER', 'DISKNUMBER', 'DISC'],
    trackNumber: ['TRACKNUMBER', 'TRACK'],
  };
}

function opusMappingProfile(): DefaultMappingProfile {
  return {
    album: ['ALBUM'],
    title: ['TITLE'],
    catalog: ['CATALOGNUMBER', 'CATALOG'],
    composer: ['COMPOSER', 'WORKCOMPOSER', 'COMPOSERSORT'],
    genre: ['GENRE'],
    conductor: ['CONDUCTOR'],
    ensemble: ['ENSEMBLE', 'ORCHESTRA', 'ALBUMARTIST', 'ARTIST'],
    soloist: ['SOLOIST', 'PERFORMER', 'ARTIST', 'ALBUMARTIST'],
    year: ['DATE', 'YEAR'],
    diskNumber: ['DISCNUMBER', 'DISKNUMBER', 'DISC'],
    trackNumber: ['TRACKNUMBER', 'TRACK'],
  };
}

function wavMappingProfile(): DefaultMappingProfile {
  return {
    album: ['ALBUM'],
    title: ['TITLE'],
    catalog: ['CATALOGNUMBER', 'CATALOG'],
    composer: ['COMPOSER', 'WORKCOMPOSER', 'COMPOSERSORT'],
    genre: ['GENRE'],
    conductor: ['CONDUCTOR'],
    ensemble: ['BAND', 'ORCHESTRA', 'ENSEMBLE', 'ALBUMARTIST'],
    soloist: ['SOLOIST', 'ARTIST', 'PERFORMER', 'ALBUMARTIST'],
    year: ['DATE', 'YEAR'],
    diskNumber: ['DISCNUMBER', 'DISKNUMBER', 'DISC'],
    trackNumber: ['TRACKNUMBER', 'TRACK'],
  };
}

function aiffMappingProfile(): DefaultMappingProfile {
  return {
    album: ['ALBUM'],
    title: ['TITLE'],
    catalog: ['CATALOGNUMBER', 'CATALOG'],
    composer: ['COMPOSER', 'WORKCOMPOSER', 'COMPOSERSORT'],
    genre: ['GENRE'],
    conductor: ['CONDUCTOR'],
    ensemble: ['BAND', 'ORCHESTRA', 'ENSEMBLE', 'ALBUMARTIST'],
    soloist: ['SOLOIST', 'ARTIST', 'PERFORMER', 'ALBUMARTIST'],
    year: ['DATE', 'YEAR'],
    diskNumber: ['DISCNUMBER', 'DISKNUMBER', 'DISC'],
    trackNumber: ['TRACKNUMBER', 'TRACK'],
  };
}

function mp4MappingProfile(): DefaultMappingProfile {
  return {
    album: ['©ALB', 'ALBUM'],
    title: ['©NAM', 'TITLE'],
    catalog: [
      '----:com.apple.iTunes:CATALOGNUMBER',
      '----:com.apple.iTunes:CATALOG',
      'CATALOGNUMBER',
      'CATALOG',
    ],
    composer: [
      '©WRT',
      'COMPOSER',
      '----:com.apple.iTunes:COMPOSER',
      'WORKCOMPOSER',
      'COMPOSERSORT',
    ],
    genre: ['©GEN', 'GNRE', 'GENRE'],
    conductor: ['----:com.apple.iTunes:CONDUCTOR', 'CONDUCTOR'],
    ensemble: [
      'AART',
      '----:com.apple.iTunes:ENSEMBLE',
      '----:com.apple.iTunes:ORCHESTRA',
      'ALBUMARTIST',
      'ENSEMBLE',
      'ORCHESTRA',
      '©ART',
      'ARTIST',
    ],
    soloist: [
      '©ART',
      'ARTIST',
      'AART',
      'ALBUMARTIST',
      '----:com.apple.iTunes:SOLOIST',
      '----:com.apple.iTunes:PERFORMER',
      'SOLOIST',
      'PERFORMER',
    ],
    year: ['©DAY', 'DATE', 'YEAR'],
    diskNumber: ['DISK', 'DISCNUMBER', 'DISKNUMBER', 'DISC'],
    trackNumber: ['TRKN', 'TRACKNUMBER', 'TRACK'],
  };
}

function createFieldMapping(
  format: string,
  key: string,
  label: string,
  tagPriorities: string[],
): LibraryFieldMapping {
  return {
    format: normalizeFieldMappingFormat(format),
    key,
    label,
    tagPriorities,
  };
}