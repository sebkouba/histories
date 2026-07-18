import { useCallback, useEffect, useState } from "react";

export const PREVIEW_MIN_WIDTH = 380;
const PREVIEW_DEFAULT_WIDTH = 510;
const PREVIEW_MAX_WIDTH = 760;
const LIBRARY_MIN_WIDTH = 560;
const STORAGE_KEY = "histories.preview-width";

function availableMaxWidth() {
  return Math.max(
    PREVIEW_MIN_WIDTH,
    Math.min(PREVIEW_MAX_WIDTH, window.innerWidth - LIBRARY_MIN_WIDTH),
  );
}

function clampWidth(width: number, maxWidth = availableMaxWidth()) {
  return Math.min(Math.max(width, PREVIEW_MIN_WIDTH), maxWidth);
}

function initialWidth() {
  const stored = Number(window.localStorage.getItem(STORAGE_KEY));
  return clampWidth(Number.isFinite(stored) && stored > 0 ? stored : PREVIEW_DEFAULT_WIDTH);
}

export function usePreviewPaneWidth() {
  const [maxWidth, setMaxWidth] = useState(availableMaxWidth);
  const [width, setStoredWidth] = useState(initialWidth);

  const setWidth = useCallback((nextWidth: number) => {
    setStoredWidth(clampWidth(nextWidth));
  }, []);

  useEffect(() => {
    const fitToWindow = () => {
      const nextMaxWidth = availableMaxWidth();
      setMaxWidth(nextMaxWidth);
      setStoredWidth((current) => clampWidth(current, nextMaxWidth));
    };
    window.addEventListener("resize", fitToWindow);
    return () => window.removeEventListener("resize", fitToWindow);
  }, []);

  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, String(Math.round(width)));
  }, [width]);

  return { maxWidth, minWidth: PREVIEW_MIN_WIDTH, setWidth, width };
}
