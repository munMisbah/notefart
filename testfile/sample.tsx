// TSX visual fixture; React is not a dependency of the Rust editor.
import { useState } from "react";

type NoteCardProps = {
  title: string;
  initialCount?: number;
};

export function NoteCard({ title, initialCount = 0 }: NoteCardProps) {
  const [count, setCount] = useState<number>(initialCount);
  const selected = count > 0;

  return (
    <article className={selected ? "note selected" : "note"}>
      {/* JSX comment and embedded expression */}
      <h1>{title}</h1>
      <p>Clicks: {count}</p>
      <button type="button" onClick={() => setCount(count + 1)}>
        Add one
      </button>
    </article>
  );
}
