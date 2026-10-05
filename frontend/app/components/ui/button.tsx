import type { ComponentProps } from "react";
import { Button as BaseButton } from "@base-ui/react/button";

import { focusRingOutset } from "./styles";

const VARIANTS = {
  primary: "bg-ink text-on-solid hover:bg-ink/85",
  quiet: "text-muted hover:bg-subtle hover:text-ink",
  danger: "bg-danger text-on-solid hover:bg-danger/85",
};

type ButtonProps = ComponentProps<typeof BaseButton> & { variant?: keyof typeof VARIANTS };

export function Button({ variant = "primary", className = "", ...props }: ButtonProps) {
  return <BaseButton className={`ui-button h-9 cursor-pointer px-4 data-disabled:cursor-not-allowed ${focusRingOutset} ${VARIANTS[variant]} ${className}`} {...props} />;
}
