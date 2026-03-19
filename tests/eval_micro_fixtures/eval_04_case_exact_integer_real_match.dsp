comparatorDes(x0, x1) = max(x0, x1) , min(x0, x1);
comparatorAsc(x0, x1) = min(x0, x1) , max(x0, x1);
comparator(0, x0, x1) = comparatorDes(x0, x1);
comparator(1, x0, x1) = comparatorAsc(x0, x1);
dir(i) = (i % (2 ^ 1) < (2 ^ 0));
process = comparator(dir(0));
