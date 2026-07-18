import { useEffect, useRef, useState } from "react";

interface PaneDividerProps {
  maxWidth: number;
  minWidth: number;
  onResize: (width: number) => void;
  width: number;
}

export function PaneDivider({ maxWidth, minWidth, onResize, width }: PaneDividerProps) {
  const dragging = useRef(false);
  const [isDragging, setIsDragging] = useState(false);

  useEffect(() => {
    if (!isDragging) return;
    document.body.classList.add("pane-resizing");
    return () => document.body.classList.remove("pane-resizing");
  }, [isDragging]);

  function resizeFromPointer(clientX: number) {
    onResize(window.innerWidth - clientX);
  }

  function startDragging(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    dragging.current = true;
    setIsDragging(true);
    event.currentTarget.setPointerCapture(event.pointerId);
    resizeFromPointer(event.clientX);
  }

  function continueDragging(event: React.PointerEvent<HTMLDivElement>) {
    if (dragging.current) resizeFromPointer(event.clientX);
  }

  function stopDragging(event: React.PointerEvent<HTMLDivElement>) {
    if (!dragging.current) return;
    dragging.current = false;
    setIsDragging(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  function resizeWithKeyboard(event: React.KeyboardEvent<HTMLDivElement>) {
    const widths: Partial<Record<string, number>> = {
      ArrowLeft: width + 24,
      ArrowRight: width - 24,
      Home: minWidth,
      End: maxWidth,
    };
    const nextWidth = widths[event.key];
    if (nextWidth === undefined) return;
    event.preventDefault();
    onResize(nextWidth);
  }

  return (
    <div
      aria-label="Resize conversation preview"
      aria-orientation="vertical"
      aria-valuemax={maxWidth}
      aria-valuemin={minWidth}
      aria-valuenow={Math.round(width)}
      className={`pane-divider ${isDragging ? "dragging" : ""}`}
      onKeyDown={resizeWithKeyboard}
      onLostPointerCapture={() => {
        dragging.current = false;
        setIsDragging(false);
      }}
      onPointerCancel={stopDragging}
      onPointerDown={startDragging}
      onPointerMove={continueDragging}
      onPointerUp={stopDragging}
      role="separator"
      tabIndex={0}
      title="Drag to resize the preview"
    >
      <span aria-hidden="true" />
    </div>
  );
}
