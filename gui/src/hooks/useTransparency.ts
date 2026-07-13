import { useEffect, useState } from "react";

type Transparency = "full" | "reduced";

export function useTransparency() {
  const [transparency, setTransparency] = useState<Transparency>(() =>
    localStorage.getItem("transparency") === "reduced" ? "reduced" : "full"
  );

  useEffect(() => {
    document.documentElement.dataset.transparency = transparency;
    localStorage.setItem("transparency", transparency);
  }, [transparency]);

  return {
    transparency,
    toggleTransparency: () => setTransparency((value) => value === "full" ? "reduced" : "full"),
  };
}
