// C++ `-vec` retains recursive state only for the first two slots; the final
// pair stays direct input/output pass-through.
process = (_*0.5,_*0.5,_,_)~(_,_);
