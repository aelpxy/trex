import { useState, type ComponentProps } from "react";
import { Button } from "@base-ui/react/button";
import { Input } from "@base-ui/react/input";
import { LuEye, LuEyeOff } from "react-icons/lu";

import { focusRing, iconButton } from "~/components/ui/styles";

export function PasswordInput({ className = "", ...props }: ComponentProps<typeof Input>) {
  const [visible, setVisible] = useState(false);

  return (
    <div className="relative">
      <Input {...props} type={visible ? "text" : "password"} className={`ui-input h-10 pr-11 ${focusRing} ${className}`} />
      <Button
        onClick={() => setVisible((value) => !value)}
        aria-label={visible ? "Hide password" : "Show password"}
        aria-pressed={visible}
        className={`${iconButton} absolute top-1 right-1`}
      >
        {visible ? <LuEyeOff size={15} /> : <LuEye size={15} />}
      </Button>
    </div>
  );
}
