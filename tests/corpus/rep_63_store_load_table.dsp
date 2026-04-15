// Test StoreTable + LoadTable: write a value into a table and read it back
N = 8;
// Counter mod N for table index
counter = (+(1)) ~ %(N);
// Write beat index into table cell 0
tbl = rdtable(N, 0.0, _);
// Output value at index 0 after writing
process = +(1.0) ~ _ : *(0.1) : tbl ~ !;
