#include "fstapi.h"

#include <iostream>

int main(int argc, char* argv[]) {
    if (argc < 2) {
        std::cerr << "Usage: ./fst <file.fst>\n";
        return 1;
    }
    std::cerr << "Opening " << argv[1] << "\n";
    void* ctx = fstReaderOpen(argv[1]);
    if (ctx == nullptr) {
        std::cerr << "Error opening file\n";
        return 2;
    }

    std::cerr << "Reading hierarchy\n";

    fstHier* hier = fstReaderIterateHier(ctx);
    if (hier == nullptr) {
        std::cerr << "Hierarchy iteration error\n";
        return 3;
    }

    std::cout << "MaxHandle: " << fstReaderGetMaxHandle(ctx) << "\n";
    std::cout << "VarCount: " << fstReaderGetVarCount(ctx) << "\n";

    // Read everything.
    fstReaderSetUnlimitedTimeRange(ctx);
    // fstReaderSetFacProcessMaskAll(ctx);
    // fstReaderSetFacProcessMask(ctx, 0+1);
    fstReaderSetFacProcessMask(ctx, 1+1);

    std::cerr << "Reading blocks\n";

    int rc = fstReaderIterBlocks2(
        ctx,
        // value_change_callback
        [](void *user_callback_data_pointer, uint64_t time, fstHandle facidx, const unsigned char *value) {
            std::cout << "Time: " << time << " id: " << facidx << " value: " << value << "\n";
        },
        // value_change_callback_varlen
        [](void *user_callback_data_pointer, uint64_t time, fstHandle facidx, const unsigned char *value, uint32_t len) {
            // TODO: Why?

        },
        // user_callback_data_pointer
        nullptr,
        // vcdhandle (if given it will write out the data to a .vcd file)
        nullptr
    );

    // Currently it always returns 1 unless ctx is null.
    if (rc != 1) {
        std::cerr << "Block iteration error\n";
        return 4;
    }

}
