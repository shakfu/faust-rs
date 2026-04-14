process = vgroup("sum", fad((*(hslider("a", 0.5, 0, 1, 0.01)), *(hslider("b", 0.5, 0, 1, 0.01))) : +))~vgroup("fb", fad(*(hslider("g", 0.5, 0, 1, 0.01))));
