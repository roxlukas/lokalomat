/** SVG path of a QR code from its module matrix (`1` = dark), with a quiet zone of `margin` modules. */
export function qrPath(size: number, modules: string, margin = 4): string {
  const parts: string[] = [];
  for (let y = 0; y < size; y++) {
    let x = 0;
    while (x < size) {
      if (modules[y * size + x] !== '1') {
        x++;
        continue;
      }
      // Merge a horizontal run of dark modules into one rectangle.
      let run = 1;
      while (x + run < size && modules[y * size + x + run] === '1') run++;
      parts.push(`M${x + margin} ${y + margin}h${run}v1h-${run}z`);
      x += run;
    }
  }
  return parts.join('');
}
