// Read-only headless backend for the existing GhidraMCP SSE bridge.
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.program.model.listing.*;
import ghidra.program.model.address.Address;
import com.sun.net.httpserver.HttpServer;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

public class HeadlessBridge extends GhidraScript {
    public void run() throws Exception {
        DecompInterface decomp = new DecompInterface();
        decomp.openProgram(currentProgram);
        HttpServer server = HttpServer.create(new InetSocketAddress("127.0.0.1", 8080), 0);
        server.createContext("/", exchange -> {
            String result;
            try {
                Map<String, String> q = new HashMap<>();
                String query = exchange.getRequestURI().getRawQuery();
                if (query != null) for (String pair : query.split("&")) {
                    String[] kv = pair.split("=", 2);
                    q.put(kv[0], URLDecoder.decode(kv.length > 1 ? kv[1] : "", "UTF-8"));
                }
                String path = exchange.getRequestURI().getPath();
                StringBuilder out = new StringBuilder();
                FunctionManager fm = currentProgram.getFunctionManager();
                if (path.equals("/get_current_function")) {
                    out.append("Headless program: ").append(currentProgram.getName());
                } else if (path.equals("/decompile") || path.equals("/decompile_function") || path.equals("/get_function_by_address") || path.equals("/disassemble_function")) {
                    Function f = null;
                    if (path.equals("/decompile")) {
                        String name = new String(exchange.getRequestBody().readAllBytes(), StandardCharsets.UTF_8);
                        for (Function candidate : fm.getFunctions(true)) if (candidate.getName().equals(name)) { f = candidate; break; }
                    } else f = fm.getFunctionContaining(toAddr(q.get("address")));
                    if (f == null) out.append("Error: function not found");
                    else if (path.equals("/get_function_by_address")) out.append(f.getName()).append(" @ ").append(f.getEntryPoint());
                    else if (path.equals("/disassemble_function")) {
                        for (Instruction ins : currentProgram.getListing().getInstructions(f.getBody(), true)) out.append(ins.getAddress()).append(": ").append(ins).append('\n');
                    } else {
                        var res = decomp.decompileFunction(f, 45, monitor);
                        out.append(res.decompileCompleted() ? res.getDecompiledFunction().getC() : res.getErrorMessage());
                    }
                } else if (path.equals("/xrefs_to")) {
                    var refs = currentProgram.getReferenceManager().getReferencesTo(toAddr(q.get("address")));
                    while (refs.hasNext()) { var ref = refs.next(); out.append(ref.getFromAddress()).append(" ").append(fm.getFunctionContaining(ref.getFromAddress())).append('\n'); }
                } else if (path.equals("/strings")) {
                    String filter = q.getOrDefault("filter", "").toLowerCase();
                    int offset = Integer.parseInt(q.getOrDefault("offset", "0")), limit = Integer.parseInt(q.getOrDefault("limit", "200")), count = 0;
                    for (Data data : currentProgram.getListing().getDefinedData(true)) if (data.hasStringValue() && data.getDefaultValueRepresentation().toLowerCase().contains(filter)) {
                        if (count++ < offset) continue; if (count > offset + limit) break;
                        out.append(data.getAddress()).append(": ").append(data.getDefaultValueRepresentation()).append('\n');
                    }
                } else if (path.equals("/methods") || path.equals("/searchFunctions") || path.equals("/list_functions")) {
                    int offset = Integer.parseInt(q.getOrDefault("offset", "0")), limit = Integer.parseInt(q.getOrDefault("limit", "100")), count = 0;
                    for (Function f : fm.getFunctions(true)) if (f.getName().contains(q.getOrDefault("query", ""))) {
                        if (count++ < offset) continue; if (count > offset + limit) break;
                        out.append(f.getEntryPoint()).append(" ").append(f.getName()).append('\n');
                    }
                } else out.append("Error: endpoint unsupported by read-only headless adapter");
                result = out.toString();
            } catch (Exception e) { result = "Error: " + e; }
            byte[] bytes = result.getBytes(StandardCharsets.UTF_8);
            exchange.sendResponseHeaders(200, bytes.length);
            exchange.getResponseBody().write(bytes); exchange.close();
        });
        server.start();
        println("PHANTOM_MCP_READY " + currentProgram.getName());
        while (!monitor.isCancelled()) Thread.sleep(1000);
        server.stop(0); decomp.dispose();
    }
}
