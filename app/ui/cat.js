// Временный статичный котик из прототипа (S03).
// Анимация и общий компонент котика — на шаге S06.
(function () {
  const HEAD = ["w.....w", "wwwwwww", "wkwwwkw", "wwwpwww", ".wwwww."];
  const BODY = [".wwwww.", "wwwwwww", "wgwwwgw"];
  const TAIL = [[7, 2], [8, 1], [8, 0]];
  const COLORS = { w: "#ffffff", g: "#d9d6cf", k: "#2b2a28", p: "#f2a7b0" };

  const px = new Map();
  BODY.forEach((row, y) => [...row].forEach((c, x) => c !== "." && px.set(`${x},${y + 5}`, c)));
  TAIL.forEach(([x, y]) => px.set(`${x},${y + 5}`, "w"));
  HEAD.forEach((row, y) => [...row].forEach((c, x) => c !== "." && px.set(`${x},${y}`, c)));

  const ctx = document.getElementById("cat").getContext("2d");
  for (const [key, c] of px) {
    const [x, y] = key.split(",").map(Number);
    ctx.fillStyle = COLORS[c];
    ctx.fillRect(x + 5, y + 4, 1, 1);
  }
})();
