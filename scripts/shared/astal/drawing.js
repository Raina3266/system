export function hexRgb(colour) {
    return [1, 3, 5].map(offset => parseInt(colour.slice(offset, offset + 2), 16) / 255);
}

// Rect coordinates exclude the stroke inset. Callers choose cut clamping and
// allocation, so small panels and glow-bearing overlays keep their own geometry.
export function cutCornerPath(cr, { x, y, width, height, cut }) {
    cr.moveTo(x + cut, y);
    cr.lineTo(x + width, y);
    cr.lineTo(x + width, y + height - cut);
    cr.lineTo(x + width - cut, y + height);
    cr.lineTo(x, y + height);
    cr.lineTo(x, y + cut);
    cr.closePath();
}

export function drawCutCornerFrame(cr, {
    x = 0.5, y = 0.5, width, height, cut = 10, color, opacity = 0.9,
}) {
    cutCornerPath(cr, { x, y, width, height, cut });
    cr.setSourceRGBA(37 / 255, 37 / 255, 47 / 255, opacity);
    cr.fillPreserve();
    cr.setSourceRGBA(...hexRgb(color), 1);
    cr.setLineWidth(1);
    cr.stroke();
}
