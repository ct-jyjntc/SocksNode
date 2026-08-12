import * as React from "react";
import { Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";

export function Spinner({
  className,
  size = "sm",
}: {
  className?: string;
  size?: "xs" | "sm" | "md";
}) {
  return (
    <Loader2
      className={cn(
        "animate-spin text-current",
        size === "xs" && "size-3",
        size === "sm" && "size-3.5",
        size === "md" && "size-5",
        className,
      )}
      strokeWidth={1.8}
      aria-hidden
    />
  );
}

export function Button({
  className,
  variant = "default",
  size = "default",
  loading = false,
  disabled,
  children,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "default" | "secondary" | "outline" | "ghost" | "destructive";
  size?: "default" | "sm" | "icon" | "icon-sm";
  loading?: boolean;
}) {
  return (
    <button
      className={cn(
        "inline-flex items-center justify-center gap-1.5 rounded-full font-medium transition-colors disabled:pointer-events-none disabled:opacity-45",
        size === "default" && "h-8 px-3 text-xs",
        size === "sm" && "h-7 px-2.5 text-xs",
        size === "icon" && "size-8",
        size === "icon-sm" && "size-7",
        variant === "default" &&
          "bg-primary text-primary-foreground hover:bg-primary/84",
        variant === "secondary" &&
          "bg-secondary text-secondary-foreground hover:bg-secondary/80",
        variant === "outline" &&
          "border border-input bg-background hover:bg-secondary/55",
        variant === "ghost" &&
          "hover:bg-secondary/55 text-muted-foreground hover:text-foreground",
        variant === "destructive" &&
          "bg-destructive text-white hover:bg-destructive/90",
        className,
      )}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...props}
    >
      {loading ? (
        <Spinner size={size === "sm" || size === "icon-sm" ? "xs" : "sm"} />
      ) : null}
      {children}
    </button>
  );
}

export function Input({
  className,
  ...props
}: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn(
        "flex h-8 w-full rounded-md border border-input bg-secondary/55 px-2.5 text-xs outline-none transition-colors placeholder:text-muted-foreground focus:bg-background focus:ring-1 focus:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export function Label({
  className,
  ...props
}: React.LabelHTMLAttributes<HTMLLabelElement>) {
  return (
    <label
      className={cn("text-xs font-normal text-muted-foreground", className)}
      {...props}
    />
  );
}

export function Badge({
  className,
  tone = "default",
  ...props
}: React.HTMLAttributes<HTMLSpanElement> & {
  tone?: "default" | "success" | "danger" | "muted" | "warn";
}) {
  return (
    <span
      className={cn(
        "inline-flex h-5 items-center gap-1 rounded-full px-2 text-[11px] font-normal",
        tone === "default" && "bg-primary/10 text-foreground",
        tone === "success" && "bg-success/15 text-success",
        tone === "danger" && "bg-destructive/10 text-destructive",
        tone === "muted" && "bg-secondary text-muted-foreground",
        tone === "warn" && "bg-amber-500/10 text-amber-700 dark:text-amber-400",
        className,
      )}
      {...props}
    />
  );
}

export function Card({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={cn("rounded-lg bg-card p-4", className)} {...props} />
  );
}

export function Checkbox({
  className,
  checked,
  onCheckedChange,
  id,
  label,
  disabled,
}: {
  className?: string;
  checked?: boolean;
  onCheckedChange?: (v: boolean) => void;
  id: string;
  label: string;
  disabled?: boolean;
}) {
  return (
    <label
      htmlFor={id}
      className={cn(
        "inline-flex cursor-pointer items-center gap-2 text-xs text-foreground",
        disabled && "cursor-not-allowed opacity-50",
        className,
      )}
    >
      <input
        id={id}
        type="checkbox"
        disabled={disabled}
        className="size-3.5 rounded border-input accent-foreground"
        checked={checked}
        onChange={(e) => onCheckedChange?.(e.target.checked)}
      />
      <span className="text-muted-foreground">{label}</span>
    </label>
  );
}

export function Dialog({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  busy = false,
  className,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: React.ReactNode;
  footer?: React.ReactNode;
  busy?: boolean;
  className?: string;
}) {
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <button
        type="button"
        className="absolute inset-0 bg-black/20 backdrop-blur-[1px]"
        aria-label="关闭"
        onClick={() => {
          if (!busy) onClose();
        }}
      />
      <div
        className={cn(
          "relative z-10 flex max-h-[min(860px,calc(100vh-2rem))] w-full max-w-[480px] flex-col rounded-lg border bg-background p-5 shadow-xl",
          className,
        )}
      >
        <div className="mb-4 shrink-0">
          <h2 className="text-sm font-medium">{title}</h2>
          {description ? (
            <p className="mt-1 text-xs text-muted-foreground">{description}</p>
          ) : null}
        </div>
        <div
          className={cn(
            "min-h-0 flex-1 space-y-4 overflow-y-auto",
            busy && "pointer-events-none opacity-70",
          )}
        >
          {children}
        </div>
        {footer ? (
          <div className="mt-5 flex shrink-0 items-center justify-end gap-2">
            {footer}
          </div>
        ) : null}
        {busy ? (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center rounded-lg bg-background/40">
            <div className="inline-flex items-center gap-2 rounded-full border bg-background px-3 py-1.5 text-xs shadow-sm">
              <Spinner size="sm" />
              处理中…
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

export function TableSkeleton({ rows = 4 }: { rows?: number }) {
  return (
    <div className="space-y-2 px-4 py-6">
      {Array.from({ length: rows }).map((_, i) => (
        <div
          key={i}
          className="h-12 animate-pulse rounded-md bg-secondary/70"
          style={{ animationDelay: `${i * 80}ms` }}
        />
      ))}
    </div>
  );
}
