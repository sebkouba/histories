type IconProps = { size?: number };

function IconFrame({ size = 18, children }: IconProps & { children: React.ReactNode }) {
  return (
    <svg
      aria-hidden="true"
      fill="none"
      height={size}
      viewBox="0 0 24 24"
      width={size}
      xmlns="http://www.w3.org/2000/svg"
    >
      {children}
    </svg>
  );
}

export function SearchIcon(props: IconProps) {
  return <IconFrame {...props}><circle cx="11" cy="11" r="7" /><path d="m16.2 16.2 4 4" /></IconFrame>;
}

export function RefreshIcon(props: IconProps) {
  return <IconFrame {...props}><path d="M20 6v5h-5" /><path d="M4 18v-5h5" /><path d="M18.2 9A7 7 0 0 0 6.7 6.7L4 11M20 13l-2.7 4.3A7 7 0 0 1 5.8 15" /></IconFrame>;
}

export function CopyIcon(props: IconProps) {
  return <IconFrame {...props}><rect x="8" y="8" width="11" height="11" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" /></IconFrame>;
}

export function PathIcon(props: IconProps) {
  return <IconFrame {...props}><path d="M4 7.5A2.5 2.5 0 0 1 6.5 5H10l2 2h5.5A2.5 2.5 0 0 1 20 9.5v7A2.5 2.5 0 0 1 17.5 19h-11A2.5 2.5 0 0 1 4 16.5Z" /><path d="M9 13h6m-2-2 2 2-2 2" /></IconFrame>;
}

export function RevealIcon(props: IconProps) {
  return <IconFrame {...props}><path d="M14 5h5v5" /><path d="m19 5-8 8" /><path d="M17 13v5a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1h5" /></IconFrame>;
}

export function ImportIcon(props: IconProps) {
  return <IconFrame {...props}><path d="M12 3v11" /><path d="m8 10 4 4 4-4" /><path d="M5 17v2h14v-2" /></IconFrame>;
}

export function CloseIcon(props: IconProps) {
  return <IconFrame {...props}><path d="m6 6 12 12M18 6 6 18" /></IconFrame>;
}

export function ChevronIcon(props: IconProps) {
  return <IconFrame {...props}><path d="m9 6 6 6-6 6" /></IconFrame>;
}

