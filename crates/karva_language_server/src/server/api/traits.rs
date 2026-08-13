use lsp_types::Request;

use crate::session::Session;

pub(super) trait RequestHandler {
    type RequestType: Request;
}

pub(super) trait SyncRequestHandler: RequestHandler {
    fn run(
        session: &mut Session,
        params: <<Self as RequestHandler>::RequestType as Request>::Params,
    ) -> anyhow::Result<<<Self as RequestHandler>::RequestType as Request>::Result>;
}
