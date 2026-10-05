/** A count with its noun in the right number: "1 component", "2 components", "1,200 events". */
export const counted = (count: number, one: string, many = `${one}s`) =>
  `${count.toLocaleString('en-GB')} ${count === 1 ? one : many}`
