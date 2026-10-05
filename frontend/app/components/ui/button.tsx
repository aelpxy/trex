import type { ComponentProps } from "react";
import { Button as BaseButton } from "@base-ui/react/button";

import { focusRingOutset } from "./styles";

const VARIANTS = {
  primary: "bg-ink text-on-solid hover:bg-ink/85",
  quiet: "text-muted hover:bg-subtle hover:text-ink",
  danger: "bg-danger text-on-solid hover:bg-danger/85",
  subtleDanger: "text-danger hover:bg-danger/10",
};

// lg beside inputs (which are h-10), md for forms and dialogs, sm for toolbars and panels, xs for
// actions inside the conversation
const SIZES = {
  lg: "h-10 px-5",
  md: "h-9 px-4",
  sm: "h-8 px-3 text-xs",
  xs: "h-7 px-2.5 text-xs",
};

type ButtonProps = ComponentProps<typeof BaseButton> & { variant?: keyof typeof VARIANTS; size?: keyof typeof SIZES };

export function Button({ variant = "primary", size = "md", className = "", ...props }: ButtonProps) {
  return <BaseButton className={`ui-button cursor-pointer data-disabled:cursor-not-allowed ${SIZES[size]} ${focusRingOutset} ${VARIANTS[variant]} ${className}`} {...props} />;
}
