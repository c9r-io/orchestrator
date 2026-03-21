interface Props {
  finished: number;
  total: number;
}

export default function ProgressBar({ finished, total }: Props) {
  const pct = total > 0 ? Math.round((finished / total) * 100) : 0;

  return (
    <div
      role="progressbar"
      aria-valuenow={finished}
      aria-valuemin={0}
      aria-valuemax={total}
      aria-label={`${finished} of ${total} completed`}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
      }}
    >
      <div
        style={{
          flex: 1,
          height: 8,
          borderRadius: 4,
          background: "var(--bg-tertiary)",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            width: `${pct}%`,
            height: "100%",
            borderRadius: 4,
            background: "var(--accent)",
            transition: "width 0.3s ease",
          }}
        />
      </div>
      <span style={{ fontSize: 13, color: "var(--text-secondary)", minWidth: 40 }}>
        {finished}/{total}
      </span>
    </div>
  );
}
