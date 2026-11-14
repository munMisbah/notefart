// TypeScript: types, generics, optional properties and template strings.
export type Theme = "dark" | "light";

export interface Note {
  readonly id: number;
  title: string;
  pinned?: boolean;
}

export function formatNote<T extends Note>(note: T, theme: Theme): string {
  const prefix = note.pinned ? "★" : "-";
  return `${prefix} ${note.title} (${theme})`;
}

const notes: Note[] = [
  { id: 1, title: "Welcome to Notefart", pinned: true },
  { id: 2, title: "Compare syntax colors" },
];

for (const note of notes) {
  console.log(formatNote(note, "dark"));
}
