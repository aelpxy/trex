import { AnimatePresence, motion } from "motion/react";
import { LuArrowDown } from "react-icons/lu";

import { focusRingOutset } from "~/components/ui/styles";

type ScrollToBottomProps = { visible: boolean; onClick: () => void };

export function ScrollToBottom({ visible, onClick }: ScrollToBottomProps) {
  return (
    <AnimatePresence>
      {visible && (
        <motion.button
          type="button"
          onClick={onClick}
          aria-label="Scroll to bottom"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: 6 }}
          transition={{ duration: 0.15 }}
          className={`glass absolute -top-11 left-1/2 flex size-8 -translate-x-1/2 cursor-pointer items-center justify-center rounded-full border border-line text-muted shadow-sm transition-colors hover:text-ink ${focusRingOutset}`}
        >
          <LuArrowDown size={15} />
        </motion.button>
      )}
    </AnimatePresence>
  );
}
