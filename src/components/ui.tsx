import * as React from "react";
import { cn } from "@/lib/utils";

export function Button({
  className,
  variant = "default",
  size = "default",
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "default" | "secondary" | "outline" | "ghost" | "destructive";
  size?: "default" | "sm" | "icon";
}) {
  return (
    <button
      className={cn(
        "inline-flex items-center justify-center gap-1.5 rounded-full font-medium transition-colors disabled:pointer-events-none disabled:opacity-45",
        size === "default" && "h-8 px-3 text-xs",
        size === "sm" && "h-7 px-2.5 text-xs",
        size === "icon" && "size-8",
        variant === "default" &&
          "bg-primary text-primary-foreground hover:bg-primary/84",
        variant === "secondary" &&
          "bg-secondary text-secondary-foreground hover:bg-secondary/80",
        variant === "outline" &&
          "border border-input bg-background hover:bg-secondary/55",
        variant === "ghost" && "hover:bg-secondary/55 text-muted-foreground hover:text-foreground",
        variant === "destructive" &&
          "bg-destructive text-white hover:bg-destructive/90",
        className,
      )}
      {...props}
    />
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
        "inline-flex h-5 items-center rounded-full px-2 text-[11px] font-normal",
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
    <div className={cn("rounded-lg bg-card p-4 sm:p-5", className)} {...props} />
  );
}

export function Checkbox({
  className,
  checked,
  onCheckedChange,
  id,
  label,
}: {
  className?: string;
  checked?: boolean;
  onCheckedChange?: (v: boolean) => void;
  id: string;
  label: string;
}) {
  return (
    <label
      htmlFor={id}
      className={cn(
        "inline-flex cursor-pointer items-center gap-2 text-xs text-foreground",
        className,
      )}
    >
      <input
        id={id}
        type="checkbox"
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
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: React.ReactNode;
  footer?: React.ReactNode;
}) {
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <button
        type="button"
        className="absolute inset-0 bg-black/20 backdrop-blur-[1px]"
        aria-label="关闭"
        onClick={onClose}
      />
      <div className="relative z-10 w-full max-w-[480px] rounded-lg border bg-background p-5 shadow-xl">
        <div className="mb-4">
          <h2 className="text-sm font-medium">{title}</h2>
          {description ? (
            <p className="mt-1 text-xs text-muted-foreground">{description}</p>
          ) : null}
        </div>
        <div className="space-y-4">{children}</div>
        {footer ? (
          <div className="mt-5 flex items-center justify-end gap-2">{footer}</div>
        ) : null}
      </div>
    </div>
  );
}
