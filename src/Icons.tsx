import type { JSX } from "solid-js";

export type IconName =
  | "add"
  | "arrow-down"
  | "arrow-up"
  | "chevron-down"
  | "chevron-right"
  | "chart"
  | "clock"
  | "download"
  | "edit"
  | "eye"
  | "eye-off"
  | "more"
  | "pin"
  | "search"
  | "spark"
  | "stop"
  | "timer"
  | "trash"
  | "x";

interface IconProps extends JSX.SvgSVGAttributes<SVGSVGElement> {
  name: IconName;
  size?: number;
}

export function Icon(props: IconProps) {
  const size = () => props.size ?? 16;

  const paths: Record<IconName, JSX.Element> = {
    add: (
      <>
        <path d="M12 5v14" />
        <path d="M5 12h14" />
      </>
    ),
    "arrow-down": (
      <>
        <path d="M12 5v14" />
        <path d="m18 13-6 6-6-6" />
      </>
    ),
    "arrow-up": (
      <>
        <path d="m6 11 6-6 6 6" />
        <path d="M12 5v14" />
      </>
    ),
    "chevron-down": <path d="m6 9 6 6 6-6" />,
    "chevron-right": <path d="m9 18 6-6-6-6" />,
    chart: (
      <>
        <path d="M4 19V9" />
        <path d="M10 19V5" />
        <path d="M16 19v-7" />
        <path d="M22 19H2" />
      </>
    ),
    clock: (
      <>
        <circle cx="12" cy="12" r="9" />
        <path d="M12 7v5l3 2" />
      </>
    ),
    download: (
      <>
        <path d="M12 3v12" />
        <path d="m7 10 5 5 5-5" />
        <path d="M5 21h14" />
      </>
    ),
    edit: (
      <>
        <path d="M12 20h9" />
        <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4Z" />
      </>
    ),
    eye: (
      <>
        <path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12" />
        <circle cx="12" cy="12" r="2.5" />
      </>
    ),
    "eye-off": (
      <>
        <path d="m3 3 18 18" />
        <path d="M10.6 6.2A10 10 0 0 1 12 6c6.5 0 10 6 10 6a18 18 0 0 1-2.2 3" />
        <path d="M6.7 6.7C3.6 8.5 2 12 2 12s3.5 6 10 6a9 9 0 0 0 3.3-.6" />
      </>
    ),
    more: (
      <>
        <circle cx="5" cy="12" r="1" />
        <circle cx="12" cy="12" r="1" />
        <circle cx="19" cy="12" r="1" />
      </>
    ),
    pin: (
      <>
        <path d="m12 17 5-5" />
        <path d="M16 3 21 8l-4 1-4 4-1 4-5-5 4-1 4-4Z" />
        <path d="m5 19 4-4" />
      </>
    ),
    search: (
      <>
        <circle cx="11" cy="11" r="7" />
        <path d="m20 20-4-4" />
      </>
    ),
    spark: (
      <>
        <path d="m12 3 1.2 3.8L17 8l-3.8 1.2L12 13l-1.2-3.8L7 8l3.8-1.2Z" />
        <path d="m18 14 .7 2.3L21 17l-2.3.7L18 20l-.7-2.3L15 17l2.3-.7Z" />
      </>
    ),
    stop: <rect x="6" y="6" width="12" height="12" rx="2" />,
    timer: (
      <>
        <circle cx="12" cy="13" r="8" />
        <path d="M12 9v4l2.5 2" />
        <path d="M9 2h6" />
      </>
    ),
    trash: (
      <>
        <path d="M4 7h16" />
        <path d="m9 7 .5-3h5l.5 3" />
        <path d="m6 7 1 14h10l1-14" />
        <path d="M10 11v6" />
        <path d="M14 11v6" />
      </>
    ),
    x: (
      <>
        <path d="m6 6 12 12" />
        <path d="m18 6-12 12" />
      </>
    ),
  };

  return (
    <svg
      {...props}
      width={size()}
      height={size()}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.8"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {paths[props.name]}
    </svg>
  );
}
