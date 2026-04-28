// Mixed AD with multiple seeds: outer RAD over inner FAD.
// inner fad(x*y, (x, y))         → [x*y, y, x]                       (3 outputs)
// outer rad([x*y, y, x], (x, y)) → [x*y, y, x, d/dx sum, d/dy sum]   (5 outputs)
//   d/dx sum = d/dx (x*y + y + x) = y + 1
//   d/dy sum = d/dy (x*y + y + x) = x + 1
x = hslider("x", 0.6, -2.0, 2.0, 0.001);
y = hslider("y", 0.4, -2.0, 2.0, 0.001);
process = rad(fad(x*y, (x, y)), (x, y));
