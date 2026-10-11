import { useCallback, useEffect, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { cn } from "@/lib/utils";

/**
 * Custom traffic-light style window controls (replaces native macOS buttons).
 * Vertically centered by the title bar flex layout.
 */
export function WindowControls({ className }: { className?: string }) {
  const [hovered, setHovered] = useState(false);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    let active = true;
    const updateMaximized = () => {
      void win.isMaximized()
        .then((value) => {
          if (active) setMaximized(value);
        })
        .catch(() => {});
    };
    updateMaximized();
    void win
      .onResized(updateMaximized)
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const close = useCallback(() => {
    void getCurrentWindow().close();
  }, []);
  const minimize = useCallback(() => {
    void getCurrentWindow().minimize();
  }, []);
  const toggleMax = useCallback(() => {
    void getCurrentWindow().toggleMaximize();
  }, []);

  return (
    <div
      className={cn(
        "app-no-drag flex h-full items-center gap-[7px] pl-3.5 pr-2",
        className,
      )}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <TrafficBtn
        tone="close"
        label="关闭"
        showGlyph={hovered}
        onClick={close}
      >
        {/* × */}
        <svg viewBox="0 0 12 12" className="size-[6px]" aria-hidden>
          <path
            d="M3 3l6 6M9 3L3 9"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinecap="round"
          />
        </svg>
      </TrafficBtn>
      <TrafficBtn
        tone="min"
        label="最小化"
        showGlyph={hovered}
        onClick={minimize}
      >
        {/* − */}
        <svg viewBox="0 0 12 12" className="size-[6px]" aria-hidden>
          <path
            d="M2.5 6h7"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinecap="round"
          />
        </svg>
      </TrafficBtn>
      <TrafficBtn
        tone="max"
        label={maximized ? "还原" : "最大化"}
        showGlyph={hovered}
        onClick={toggleMax}
      >
        {maximized ? (
          <svg viewBox="0 0 12 12" className="size-[6px]" aria-hidden>
            <path
              d="M4 3.5h4.5V8M3.5 4v4.5H8"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.2"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        ) : (
          <svg viewBox="0 0 12 12" className="size-[6px]" aria-hidden>
            <path
              d="M3.2 6.5V3.2H6.5M8.8 5.5v3.3H5.5"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.2"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        )}
      </TrafficBtn>
    </div>
  );
}

function TrafficBtn({
  tone,
  label,
  showGlyph,
  onClick,
  children,
}: {
  tone: "close" | "min" | "max";
  label: string;
  showGlyph: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      onMouseDown={(e) => e.stopPropagation()}
      className={cn(
        "group relative flex size-[12px] shrink-0 items-center justify-center rounded-full",
        "transition-[filter,transform] duration-100",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40",
        "active:scale-[0.92]",
        tone === "close" &&
          "bg-[#ff5f57] text-[#4d0000] hover:brightness-105",
        tone === "min" && "bg-[#febc2e] text-[#5a3a00] hover:brightness-105",
        tone === "max" && "bg-[#28c840] text-[#006500] hover:brightness-105",
      )}
    >
      <span
        className={cn(
          "pointer-events-none flex items-center justify-center transition-opacity duration-100",
          showGlyph ? "opacity-100" : "opacity-0",
        )}
      >
        {children}
      </span>
    </button>
  );
}
