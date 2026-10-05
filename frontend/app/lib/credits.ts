// credits are millionths of a US dollar
export const CREDITS_PER_DOLLAR = 1_000_000;

const CENT = CREDITS_PER_DOLLAR / 100;

// cents for anything a cent or more, more precision for the fractions of a cent single replies cost
export function formatUsd(credits: number): string {
  const dollars = credits / CREDITS_PER_DOLLAR;
  const sign = dollars < 0 ? "-" : "";
  const size = Math.abs(dollars);
  if (size === 0) return "$0.00";
  if (Math.abs(credits) >= CENT) return `${sign}$${size.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
  return `${sign}$${size.toLocaleString(undefined, { maximumSignificantDigits: 2 })}`;
}

export const dollarsToCredits = (dollars: number) => Math.round(dollars * CREDITS_PER_DOLLAR);
