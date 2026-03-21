interface Props {
  width?: number | string;
  height?: number | string;
}

/** Animated skeleton placeholder for loading states. */
export default function Skeleton({ width = "100%", height = 20 }: Props) {
  return (
    <div
      className="skeleton"
      style={{ width, height, borderRadius: 12 }}
      aria-hidden="true"
    />
  );
}
