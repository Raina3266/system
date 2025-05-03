{inputs, ...}: {
    imports = [inputs.nixvim.homeManagerModules.nixvim];
    config = {
        programs.nixvim.enable = true;
    };
}