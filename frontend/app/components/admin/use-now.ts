import { useEffect, useState } from "react";

// the current unix time in seconds, ticking so durations on screen stay live
export function useNow(everyMs = 1000) {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => {
    const timer = setInterval(() => setNow(Math.floor(Date.now() / 1000)), everyMs);
    return () => clearInterval(timer);
  }, [everyMs]);
  return now;
}
