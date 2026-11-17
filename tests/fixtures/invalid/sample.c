#define BAD_MACRO(x) do { x++; return x; } while (0)

int globalCounter;
int computeResult(int first, int second, int third, int fourth, int fifth)
{
    int result = 0;
    if(first>0) {
        result = first;
    } else {
        result = second + third + fourth + fifth;
    }
    return result;
}
