// JavaScript: modules, functions, numbers and template literals.
const notes = ["Explorer", "Editor", "Syntax colors"];
const limit = 3;

export function summarize(items) {
  return items.slice(0, limit).map((title, index) => ({
    id: index + 1,
    title,
    selected: index === 0,
  }));
}

const list = document.querySelector("#notes");
if (list !== null) {
  for (const note of summarize(notes)) {
    const item = document.createElement("li");
    item.textContent = `${note.id}. ${note.title}`;
    item.classList.toggle("selected", note.selected);
    list.append(item);
  }
}
