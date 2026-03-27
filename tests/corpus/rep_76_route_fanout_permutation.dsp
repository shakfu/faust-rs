// Route coverage beyond the arithmetic-parameter normalization case.
//
// Input 1 fans out to outputs 1 and 2; input 2 feeds output 3.

process = route(2, 3, 1,1, 1,2, 2,3);
