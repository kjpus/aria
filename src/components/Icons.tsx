import type { SVGProps } from 'react';

type IconProps = SVGProps<SVGSVGElement> & {
  size?: number | string;
};

export function AriaAppLogo({ size = 28, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      viewBox="36 36 440 440"
      width={size}
      xmlns="http://www.w3.org/2000/svg"
      {...props}
    >
      <defs>
        <linearGradient id="ariaTileBrass" x1="96" y1="72" x2="422" y2="438" gradientUnits="userSpaceOnUse">
          <stop stopColor="#2F261E" />
          <stop offset="1" stopColor="#12100F" />
        </linearGradient>
        <radialGradient
          id="ariaTileGlowWarm"
          cx="0"
          cy="0"
          gradientTransform="translate(188 144) rotate(34) scale(322)"
          gradientUnits="userSpaceOnUse"
          r="1"
        >
          <stop stopColor="#D8B26B" stopOpacity="0.3" />
          <stop offset="1" stopColor="#D8B26B" stopOpacity="0" />
        </radialGradient>
        <linearGradient id="ariaChampagneStroke" x1="176" y1="82" x2="326" y2="318" gradientUnits="userSpaceOnUse">
          <stop stopColor="#FFF3DB" />
          <stop offset="0.48" stopColor="#E9D2A4" />
          <stop offset="1" stopColor="#C08A4B" />
        </linearGradient>
        <radialGradient
          id="ariaDotPearl"
          cx="0"
          cy="0"
          gradientTransform="translate(244 102) rotate(36) scale(58)"
          gradientUnits="userSpaceOnUse"
          r="1"
        >
          <stop stopColor="#FFFFFF" />
          <stop offset="0.45" stopColor="#F5E8D3" />
          <stop offset="1" stopColor="#D2B27B" />
        </radialGradient>
      </defs>
      <rect fill="url(#ariaTileBrass)" height="432" rx="132" width="432" x="40" y="40" />
      <rect fill="url(#ariaTileGlowWarm)" height="432" rx="132" width="432" x="40" y="40" />
      <rect height="432" rx="132" stroke="#FFFFFF" strokeOpacity="0.07" strokeWidth="2" width="432" x="40" y="40" />

      <path d="M166 136L251 360" stroke="#081018" strokeLinecap="round" strokeOpacity="0.22" strokeWidth="58" />
      <path d="M346 136L261 360" stroke="#081018" strokeLinecap="round" strokeOpacity="0.22" strokeWidth="58" />
      <circle cx="256" cy="88" fill="#081018" fillOpacity="0.16" r="35" />

      <path d="M160 124L245 348" stroke="url(#ariaChampagneStroke)" strokeLinecap="round" strokeWidth="42" />
      <path d="M352 124L267 348" stroke="url(#ariaChampagneStroke)" strokeLinecap="round" strokeWidth="42" />
      <circle cx="256" cy="82" fill="url(#ariaDotPearl)" r="29" />

      <path d="M247 150L218 226" stroke="#FFFFFF" strokeLinecap="round" strokeOpacity="0.22" strokeWidth="8" />
      <path d="M275 150L304 226" stroke="#FFFFFF" strokeLinecap="round" strokeOpacity="0.18" strokeWidth="8" />
    </svg>
  );
}

export function LibraryIcon({ size = 18, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.75"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <rect height="7" rx="1.5" width="7" x="3" y="3" />
      <rect height="7" rx="1.5" width="7" x="14" y="3" />
      <rect height="7" rx="1.5" width="7" x="14" y="14" />
      <rect height="7" rx="1.5" width="7" x="3" y="14" />
    </svg>
  );
}

export function AlbumIcon({ size = 18, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.75"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <circle cx="12" cy="12" r="10" />
      <circle cx="12" cy="12" r="4" />
      <circle cx="12" cy="12" fill="currentColor" r="1.5" />
    </svg>
  );
}

export function TracksIcon({ size = 18, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.75"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <path d="M9 18V5l12-2v13" />
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="16" r="3" />
    </svg>
  );
}

export function PlaylistIcon({ size = 18, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.75"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <path d="M21 15V6" />
      <path d="M18.5 18a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z" />
      <path d="M12 12H3" />
      <path d="M16 6H3" />
      <path d="M12 18H3" />
    </svg>
  );
}

export function QueueIcon({ size = 18, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.75"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <line x1="8" x2="21" y1="6" y2="6" />
      <line x1="8" x2="21" y1="12" y2="12" />
      <line x1="8" x2="21" y1="18" y2="18" />
      <polygon fill="currentColor" points="3,6 6,8 3,10" />
      <line x1="3" x2="5" y1="12" y2="12" />
      <line x1="3" x2="5" y1="18" y2="18" />
    </svg>
  );
}

export function SettingsIcon({ size = 18, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.75"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <line x1="4" x2="4" y1="21" y2="14" />
      <line x1="4" x2="4" y1="10" y2="3" />
      <line x1="12" x2="12" y1="21" y2="12" />
      <line x1="12" x2="12" y1="8" y2="3" />
      <line x1="20" x2="20" y1="21" y2="16" />
      <line x1="20" x2="20" y1="12" y2="3" />
      <line x1="1" x2="7" y1="14" y2="14" />
      <line x1="9" x2="15" y1="8" y2="8" />
      <line x1="17" x2="23" y1="16" y2="16" />
    </svg>
  );
}

export function ArrowLeftIcon({ size = 16, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <path d="m15 18-6-6 6-6" />
    </svg>
  );
}

export function SearchIcon({ size = 16, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <circle cx="11" cy="11" r="8" />
      <path d="m21 21-4.3-4.3" />
    </svg>
  );
}

export function SortIcon({ size = 16, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <path d="m3 16 4 4 4-4" />
      <path d="M7 20V4" />
      <path d="m21 8-4-4-4 4" />
      <path d="M17 4v16" />
    </svg>
  );
}

export function GridSmallIcon({ size = 16, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <rect height="4" width="4" x="3" y="3" />
      <rect height="4" width="4" x="10" y="3" />
      <rect height="4" width="4" x="17" y="3" />
      <rect height="4" width="4" x="3" y="10" />
      <rect height="4" width="4" x="10" y="10" />
      <rect height="4" width="4" x="17" y="10" />
      <rect height="4" width="4" x="3" y="17" />
      <rect height="4" width="4" x="10" y="17" />
      <rect height="4" width="4" x="17" y="17" />
    </svg>
  );
}

export function GridMediumIcon({ size = 16, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <rect height="7" width="7" x="3" y="3" />
      <rect height="7" width="7" x="14" y="3" />
      <rect height="7" width="7" x="3" y="14" />
      <rect height="7" width="7" x="14" y="14" />
    </svg>
  );
}

export function GridLargeIcon({ size = 16, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <rect height="18" width="18" x="3" y="3" rx="2" />
    </svg>
  );
}

export function PlaySimpleIcon({ size = 16, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="currentColor"
      height={size}
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <path d="M7 5.5v13a1 1 0 0 0 1.55.83l11-6.5a1 1 0 0 0 0-1.66l-11-6.5A1 1 0 0 0 7 5.5Z" />
    </svg>
  );
}

export function CheckIcon({ size = 16, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2.5"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

export function SidebarToggleIcon({ size = 16, className, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
      viewBox="0 0 24 24"
      width={size}
      {...props}
    >
      <rect height="18" rx="2" width="18" x="3" y="3" />
      <path d="M9 3v18" />
    </svg>
  );
}

export function EmptyArtPlaceholder({ className }: { className?: string }) {
  return (
    <div className={`empty-art-placeholder${className ? ` ${className}` : ''}`}>
      <svg
        aria-hidden="true"
        fill="none"
        height="48"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="1.25"
        viewBox="0 0 64 64"
        width="48"
      >
        <circle cx="32" cy="32" r="28" stroke="currentColor" strokeOpacity="0.4" strokeWidth="1.5" />
        <circle cx="32" cy="32" r="18" stroke="currentColor" strokeOpacity="0.25" strokeWidth="1" />
        <circle cx="32" cy="32" r="10" stroke="currentColor" strokeOpacity="0.3" strokeWidth="1" />
        <circle cx="32" cy="32" fill="currentColor" fillOpacity="0.8" r="3.5" />
        {/* Stylized sound grooves */}
        <path d="M32 6 A26 26 0 0 1 54 18" stroke="currentColor" strokeDasharray="3 4" strokeOpacity="0.3" />
        <path d="M10 46 A26 26 0 0 1 18 18" stroke="currentColor" strokeDasharray="3 4" strokeOpacity="0.3" />
      </svg>
      <span className="empty-art-placeholder__label">Aria</span>
    </div>
  );
}
