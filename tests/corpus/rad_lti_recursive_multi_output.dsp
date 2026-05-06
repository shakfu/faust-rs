// Accepted RAD E1 fixture: two independent strict-LTI recursions.
//
// This pins the public multi-output layout for accepted recursive RAD:
// [y0, y1, dp, dq].
p = 0.5;
q = 0.25;
process = rad(((2 : + ~ *(p)), (3 : + ~ *(q))), (p, q));
