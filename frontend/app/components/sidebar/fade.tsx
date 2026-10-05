import type { ReactNode } from "react";
import { motion } from "motion/react";

type FadeProps = { show: boolean; as?: "span" | "div"; className?: string; children: ReactNode };

export function Fade({ show, as = "span", className = "", children }: FadeProps) {
  const Component = as === "div" ? motion.div : motion.span;
  return (
    <Component
      initial={false}
      animate={{ opacity: show ? 1 : 0 }}
      transition={show ? { duration: 0.15, delay: 0.05 } : { duration: 0.1 }}
      aria-hidden={!show}
      inert={!show}
      className={`${show ? "" : "pointer-events-none"} ${className}`}
    >
      {children}
    </Component>
  );
}
