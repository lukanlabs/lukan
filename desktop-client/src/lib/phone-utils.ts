export const COUNTRY_CODES: { value: string; label: string; code: string }[] = [
  { value: "54", label: "Argentina (+54)", code: "+54" },
  { value: "55", label: "Brazil (+55)", code: "+55" },
  { value: "56", label: "Chile (+56)", code: "+56" },
  { value: "86", label: "China (+86)", code: "+86" },
  { value: "57", label: "Colombia (+57)", code: "+57" },
  { value: "593", label: "Ecuador (+593)", code: "+593" },
  { value: "33", label: "France (+33)", code: "+33" },
  { value: "49", label: "Germany (+49)", code: "+49" },
  { value: "91", label: "India (+91)", code: "+91" },
  { value: "39", label: "Italy (+39)", code: "+39" },
  { value: "81", label: "Japan (+81)", code: "+81" },
  { value: "52", label: "Mexico (+52)", code: "+52" },
  { value: "51", label: "Peru (+51)", code: "+51" },
  { value: "82", label: "South Korea (+82)", code: "+82" },
  { value: "34", label: "Spain (+34)", code: "+34" },
  { value: "44", label: "United Kingdom (+44)", code: "+44" },
  { value: "1", label: "United States (+1)", code: "+1" },
  { value: "58", label: "Venezuela (+58)", code: "+58" },
];

const COUNTRY_CODE_PREFIXES = COUNTRY_CODES.map((c) => ({
  prefix: c.value,
  display: c.code,
})).sort((a, b) => b.prefix.length - a.prefix.length);

export function formatPhoneNumber(num: string): string {
  for (const { prefix, display } of COUNTRY_CODE_PREFIXES) {
    if (num.startsWith(prefix)) return `${display} ${num.slice(prefix.length)}`;
  }
  return `+${num}`;
}
