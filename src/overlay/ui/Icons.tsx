// Inline 24x24 stroke icons (1.75 px), drawn for this toolbar so they share one visual weight.
import type { ReactNode } from "react";

function Svg({ children }: { children: ReactNode }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

export const SelectIcon = () => (
  <Svg>
    <path d="M6 3.5v14.2l3.9-3.8 2.6 6.1 2.4-1-2.6-6h5.4z" />
  </Svg>
);

export const ArrowIcon = () => (
  <Svg>
    <path d="M5 19 18 6" />
    <path d="M9.5 6H18v8.5" />
  </Svg>
);

export const PencilIcon = () => (
  <Svg>
    <path d="M4.5 19.5 5.6 15 15.8 4.8a2.1 2.1 0 0 1 3 3L8.6 18z" />
    <path d="m13.8 6.8 3 3" />
  </Svg>
);

export const RectIcon = () => (
  <Svg>
    <rect x="4" y="6" width="16" height="12" rx="1.5" />
  </Svg>
);

export const MarkerIcon = () => (
  <Svg>
    <path d="m14.5 4.5 5 5-7.5 7.5H7v-5z" />
    <path d="M4 20h8" />
  </Svg>
);

export const BlurIcon = () => (
  <Svg>
    <rect x="4" y="4" width="16" height="16" rx="2" />
    <path d="M4 9.33h16M4 14.67h16M9.33 4v16M14.67 4v16" />
  </Svg>
);

export const SolidIcon = () => (
  <Svg>
    <rect x="4" y="6" width="16" height="12" rx="2" fill="currentColor" />
  </Svg>
);

export const MarkIcon = () => (
  <Svg>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M10.6 9.4 12.6 8v8" />
  </Svg>
);

export const TextIcon = () => (
  <Svg>
    <path d="M5 7V5h14v2M12 5v14M9.5 19h5" />
  </Svg>
);

export const UndoIcon = () => (
  <Svg>
    <path d="M9 13.5 4.5 9 9 4.5" />
    <path d="M4.5 9H15a5 5 0 0 1 0 10h-3.5" />
  </Svg>
);

export const RedoIcon = () => (
  <Svg>
    <path d="M15 13.5 19.5 9 15 4.5" />
    <path d="M19.5 9H9a5 5 0 0 0 0 10h3.5" />
  </Svg>
);

export const PlusIcon = () => (
  <Svg>
    <path d="M12 5v14M5 12h14" />
  </Svg>
);

export const SaveIcon = () => (
  <Svg>
    <path d="M12 4v11" />
    <path d="m7.5 10.5 4.5 4.5 4.5-4.5" />
    <path d="M5 19.5h14" />
  </Svg>
);

export const CopyIcon = () => (
  <Svg>
    <rect x="8.5" y="8.5" width="11" height="11" rx="2" />
    <path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2" />
  </Svg>
);

export const CloseIcon = () => (
  <Svg>
    <path d="M6.5 6.5l11 11M17.5 6.5l-11 11" />
  </Svg>
);

export const PlateIcon = () => (
  <Svg>
    <rect x="3.5" y="5.5" width="17" height="13" rx="2.5" />
    <path d="M8.5 9.5h7M12 9.5v5.5" />
  </Svg>
);

/** Callout text on the image: a picture frame with text lines inside. */
export const ImageTextIcon = () => (
  <Svg>
    <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
    <path d="M7.5 9h9M7.5 12.5h6" />
    <path d="m3.5 17.5 4.5-3 3 2 3-2.5 6.5 4.5" />
  </Svg>
);

/** Callout text kept as separate text. */
export const TextLinesIcon = () => (
  <Svg>
    <path d="M5 7h14M5 12h14M5 17h9" />
  </Svg>
);

/** Caption after drawing: a numbered badge with text lines and a pointer. */
export const CaptionIcon = () => (
  <Svg>
    <circle cx="6.5" cy="7" r="3.5" />
    <path d="M13 6h7M13 10h5M8.5 10l7 9" />
  </Svg>
);

/** Width preview: a short stroke of the given thickness in icon units. */
export const WidthIcon = ({ weight }: { weight: number }) => (
  <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
    <path d="M5 12h14" stroke="currentColor" strokeWidth={weight} strokeLinecap="round" />
  </svg>
);
